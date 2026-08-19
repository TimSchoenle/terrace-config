//! The Kubernetes half of the protocol: what a chart stamps, and what a policy pairs it against.
//!
//! `contract.rs` checks the document a build publishes. This file checks the metadata a *chart*
//! renders, and the two properties that half has which the image half does not:
//!
//! - **Every key and value has to survive Kubernetes' own rules.** The image labels do not — the
//!   loader's prefix ends in an underscore and the contract path is full of slashes — so the one
//!   thing this module must never do is emit something an API server refuses. That failure lands
//!   at `kubectl apply` time, in a pipeline, on a value nobody looked at twice.
//! - **The pairing is only worth running if it cannot be satisfied by accident.** A membership
//!   test against a list of image references passes trivially if the list is unpinned or the
//!   comparison is loose, and a check that always passes is worse than no check, because
//!   somebody is relying on it.
//!
//! So the assertions below are mostly refusals. The round trip is one test; the rest are the ways
//! a chart can be wrong.

#![cfg(feature = "schema")]
#![expect(dead_code, reason = "fixtures are read by the derive, not at runtime")]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use terrace_config::Terrace;
use terrace_config::schema::kube::{
    ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT, ANNOTATION_IMAGES, Format, LABEL_CONTRACT_VERSION,
    Metadata, NAMESPACE, Pairing, Target,
};
use terrace_config::schema::{
    App, CONTRACT_VERSION, Contract, DEFAULT_PATH, Describe, LABEL_PREFIX, LABEL_VERSION,
};

#[derive(Deserialize, Serialize, Default, Describe)]
struct Config {
    /// Bundle directory the readiness probe checks.
    #[serde(default)]
    dist_dir: String,
    /// Revalidation interval in seconds.
    #[serde(default)]
    ttl_secs: u64,
}

/// Three digests that differ in their first byte, so a membership failure names which one.
const DIGEST_A: &str = "sha256:aa259cb1b0f1a1d3b0f6c0a2e5d4c3b2a1908f7e6d5c4b3a2918070605040300";
const DIGEST_B: &str = "sha256:bb259cb1b0f1a1d3b0f6c0a2e5d4c3b2a1908f7e6d5c4b3a2918070605040300";
const DIGEST_C: &str = "sha256:cc259cb1b0f1a1d3b0f6c0a2e5d4c3b2a1908f7e6d5c4b3a2918070605040300";
const DIGEST_D: &str = "sha256:dd259cb1b0f1a1d3b0f6c0a2e5d4c3b2a1908f7e6d5c4b3a2918070605040300";

fn image(name: &str, digest: &str) -> String {
    format!("ghcr.io/you/{name}@{digest}")
}

fn contract() -> Contract {
    Terrace::new("PORTFOLIO_")
        .schema::<Config>()
        .into_contract(App::new("portfolio").version("v2.5.0"))
        .build()
        .expect("the contract has nothing to refuse")
}

/// A document target, since almost every test below is about one.
fn document() -> Target {
    Target::document("config.toml", Format::Toml)
}

/// The metadata for a document read by one image.
fn stamped(contract: &Contract) -> Metadata {
    contract
        .kube_metadata(&document(), &[&image("portfolio", DIGEST_A)])
        .expect("a pinned reference and a legal key")
}

/// What `crane config` reports for an image built from `contract`.
fn image_labels(contract: &Contract) -> BTreeMap<String, String> {
    contract
        .labels(DEFAULT_PATH)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Kubernetes legality — the constraint the whole design is shaped by
// ---------------------------------------------------------------------------------------------

/// The rules restated here rather than called, because the module under test is the one deciding
/// what is legal and a test that asked it would agree with it by construction.
fn is_legal_label_value(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let legal = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.';
    value.len() <= 63
        && value.chars().all(legal)
        && value.starts_with(|c: char| c.is_ascii_alphanumeric())
        && value.ends_with(|c: char| c.is_ascii_alphanumeric())
}

fn is_legal_key(key: &str) -> bool {
    let (prefix, name) = match key.split_once('/') {
        Some((prefix, name)) => (Some(prefix), name),
        None => (None, key),
    };
    let prefix_ok = prefix.is_none_or(|prefix| {
        !prefix.is_empty()
            && prefix.len() <= 253
            && prefix.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && label
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                    && label.starts_with(|c: char| c.is_ascii_alphanumeric())
                    && label.ends_with(|c: char| c.is_ascii_alphanumeric())
            })
    });
    prefix_ok && !name.is_empty() && !name.contains('/') && is_legal_label_value(name)
}

#[test]
fn every_key_and_value_survives_the_kubernetes_rules_even_for_a_hostile_contract() {
    // A prefix that is illegal as a label value in two ways at once — a trailing underscore and a
    // length well past 63 — and an app name full of characters the class refuses. If any of these
    // reached a label, this is the test that says so; the design's answer is that none of them is
    // ever a label, because the only label carries the contract version.
    let hostile = Terrace::new(format!("{}_", "A".repeat(80)))
        .schema::<Config>()
        .into_contract(
            App::new("Portfolio/Server:v2.5.0+build.7 — with a name nobody would choose")
                .version("v2.5.0+build.7"),
        )
        .build()
        .expect("a hostile prefix is a contract's business, not Kubernetes'");

    for target in [document(), Target::Workload] {
        let metadata = hostile
            .kube_metadata(&target, &[&image("portfolio", DIGEST_A)])
            .expect("nothing hostile reaches a key or a value");

        for (key, value) in metadata.labels() {
            assert!(is_legal_key(key), "`{key}` is not a legal label key");
            assert!(
                is_legal_label_value(value),
                "`{key}` has the illegal label value `{value}`"
            );
        }
        // An annotation *key* obeys the label-key rule; an annotation value does not, which is
        // the whole reason the image list and the prefix live over here.
        for key in metadata.annotations().keys() {
            assert!(is_legal_key(key), "`{key}` is not a legal annotation key");
        }
    }
}

#[test]
fn the_prefix_and_the_contract_path_are_nowhere_in_the_stamp() {
    // Both are facts about an image and the image already carries them. A copy here would be a
    // second spelling of a fact that has one, with nothing able to catch it drifting.
    let contract = contract();
    let metadata = stamped(&contract);

    let stamp = format!("{metadata:?}");
    assert!(!stamp.contains("PORTFOLIO_"), "{stamp}");
    assert!(!stamp.contains(DEFAULT_PATH), "{stamp}");

    // And the only label there is, is the version.
    assert_eq!(
        metadata.labels().keys().collect::<Vec<_>>(),
        vec![LABEL_CONTRACT_VERSION]
    );
    assert_eq!(
        metadata.labels()[LABEL_CONTRACT_VERSION],
        CONTRACT_VERSION.to_string()
    );
}

// ---------------------------------------------------------------------------------------------
// The two targets
// ---------------------------------------------------------------------------------------------

#[test]
fn a_pod_template_carries_the_image_list_and_nothing_about_a_document() {
    let contract = contract();
    let workload = contract
        .kube_metadata(&Target::Workload, &[&image("portfolio", DIGEST_A)])
        .expect("a pinned reference");

    assert!(workload.annotations().contains_key(ANNOTATION_IMAGES));
    // A pod has no `data`, so a key into one is a claim about an object this is not.
    assert!(!workload.annotations().contains_key(ANNOTATION_DOCUMENT_KEY));
    assert!(!workload.annotations().contains_key(ANNOTATION_FORMAT));
    // It is stamped at all so an admission webhook holding only the pod can find the image list
    // without walking ownership references.
    assert!(workload.labels().contains_key(LABEL_CONTRACT_VERSION));

    let document = stamped(&contract);
    assert_eq!(
        document.annotations()[ANNOTATION_DOCUMENT_KEY],
        "config.toml"
    );
    assert_eq!(document.annotations()[ANNOTATION_FORMAT], "toml");
}

#[test]
fn a_document_key_on_a_pod_template_is_refused_rather_than_ignored() {
    let contract = contract();
    let workload = contract
        .kube_metadata(&Target::Workload, &[&image("portfolio", DIGEST_A)])
        .expect("a pinned reference");

    let mut annotations = workload.annotations().clone();
    annotations.insert(ANNOTATION_DOCUMENT_KEY.to_owned(), "config.toml".to_owned());

    let error = contract
        .verify_kube_metadata(&Target::Workload, workload.labels(), &annotations)
        .expect_err("a document key describes an object a pod is not");
    assert!(
        error.to_string().contains(ANNOTATION_DOCUMENT_KEY),
        "{error}"
    );
}

#[test]
fn a_misspelled_key_in_this_namespace_is_reported_and_a_foreign_one_is_ignored() {
    let contract = contract();
    let metadata = stamped(&contract);

    // What a chart actually carries, none of which is this document's business.
    let mut labels = metadata.labels().clone();
    labels.insert("app.kubernetes.io/name".to_owned(), "portfolio".to_owned());
    labels.insert("helm.sh/chart".to_owned(), "portfolio-1.2.3".to_owned());
    let mut annotations = metadata.annotations().clone();
    annotations.insert(
        "kubectl.kubernetes.io/last-applied-configuration".to_owned(),
        "{}".to_owned(),
    );
    contract
        .verify_kube_metadata(&document(), &labels, &annotations)
        .expect("foreign metadata is ignored, as the image half ignores foreign labels");

    // A key inside this namespace is a different matter: nothing else owns it, so a misspelling
    // here is one nothing would ever report.
    let mut typo = metadata.annotations().clone();
    typo.insert(format!("{NAMESPACE}/image"), image("portfolio", DIGEST_A));
    let error = contract
        .verify_kube_metadata(&document(), metadata.labels(), &typo)
        .expect_err("a near-miss in our own namespace is a misspelling nothing reads");
    assert!(
        error.to_string().contains("dev.terrace.config/image"),
        "{error}"
    );
}

// ---------------------------------------------------------------------------------------------
// Image references
// ---------------------------------------------------------------------------------------------

#[test]
fn an_unpinned_image_reference_is_refused() {
    let contract = contract();

    for unpinned in [
        "ghcr.io/you/portfolio",
        "ghcr.io/you/portfolio:v2.5.0",
        "ghcr.io/you/portfolio:latest",
    ] {
        let error = contract
            .kube_metadata(&document(), &[unpinned])
            .expect_err("a tag can be moved, so a pairing keyed on one proves nothing");
        assert!(error.to_string().contains(unpinned), "{error}");
    }

    // And on the way back in, so a hand-written chart is caught by the same rule.
    let metadata = stamped(&contract);
    let mut annotations = metadata.annotations().clone();
    annotations.insert(
        ANNOTATION_IMAGES.to_owned(),
        "ghcr.io/you/portfolio:v2.5.0".to_owned(),
    );
    assert!(
        contract
            .verify_kube_metadata(&document(), metadata.labels(), &annotations)
            .is_err()
    );
}

#[test]
fn an_object_read_by_no_image_is_refused() {
    // A stamp with an empty list claims to participate and gives a validator nothing to pair
    // against: every membership test against an empty list fails.
    let error = contract()
        .kube_metadata(&document(), &[])
        .expect_err("an empty image list is a stamp that cannot be checked");
    assert!(error.to_string().contains(ANNOTATION_IMAGES), "{error}");
}

// ---------------------------------------------------------------------------------------------
// The pairing
// ---------------------------------------------------------------------------------------------

#[test]
fn each_of_a_documents_readers_pairs_with_it_and_a_stranger_does_not() {
    // The union case: one rendered document, one prefix, three binaries, each `Describe` covering
    // only the keys it consumes. Membership rather than equality is what makes this deployable.
    let contract = contract();
    let readers = [
        image("portfolio", DIGEST_A),
        image("sidecar", DIGEST_B),
        image("migrator", DIGEST_C),
    ];
    let refs: Vec<&str> = readers.iter().map(String::as_str).collect();
    let metadata = contract
        .kube_metadata(&document(), &refs)
        .expect("three pinned references");
    let labels = image_labels(&contract);

    for reader in &readers {
        Pairing::new(&contract)
            .image(&labels, reader)
            .object(metadata.labels(), metadata.annotations())
            .check()
            .unwrap_or_else(|e| panic!("`{reader}` is one of this document's readers: {e}"));
    }

    let stranger = image("paperless", DIGEST_D);
    let error = Pairing::new(&contract)
        .image(&labels, &stranger)
        .object(metadata.labels(), metadata.annotations())
        .check()
        .expect_err("a container reading a document it is not listed in");
    // Both sides named, so the reader knows whether to fix the chart or the mount.
    assert!(error.to_string().contains(&stranger), "{error}");
    assert!(error.to_string().contains(&readers[0]), "{error}");
}

#[test]
fn a_tag_beside_the_digest_still_pairs_with_the_digest_alone() {
    // A chart pins `image.tag: v2.5.0@sha256:…` for whoever reads the values file, while a pod's
    // `imageID` carries no tag at all. Comparing the whole string would refuse a pairing that is
    // correct, and refuse it pointing at a digest that visibly matches.
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[&image("portfolio", DIGEST_A)])
        .expect("a pinned reference");

    Pairing::new(&contract)
        .image(
            &image_labels(&contract),
            &format!("ghcr.io/you/portfolio:v2.5.0@{DIGEST_A}"),
        )
        .object(metadata.labels(), metadata.annotations())
        .check()
        .expect("the digest is what resolves, and it is the same digest");
}

#[test]
fn a_version_skew_between_the_object_and_the_image_names_both_sides() {
    let contract = contract();
    let metadata = stamped(&contract);

    // The chart and the image rolled out separately: the object was rendered against a contract
    // one version behind the image that is about to read it.
    let mut labels = metadata.labels().clone();
    labels.insert(LABEL_CONTRACT_VERSION.to_owned(), "0".to_owned());

    let error = Pairing::new(&contract)
        .image(&image_labels(&contract), &image("portfolio", DIGEST_A))
        .object(&labels, metadata.annotations())
        .check()
        .expect_err("an object a version behind the image reading it");
    let message = error.to_string();
    assert!(message.contains(LABEL_CONTRACT_VERSION), "{message}");
    assert!(message.contains('0'), "{message}");
    assert!(
        message.contains(&CONTRACT_VERSION.to_string()),
        "the contract's own version is the other side: {message}"
    );

    // The same skew seen from the image, which is the question that does not go through the
    // contract at all: here the object agrees with the contract and the image does not.
    let mut image = image_labels(&contract);
    image.insert(LABEL_VERSION.to_owned(), "99".to_owned());
    let error = Pairing::new(&contract)
        .image(&image, &self::image("portfolio", DIGEST_A))
        .object(metadata.labels(), metadata.annotations())
        .check()
        .expect_err("an image a version ahead of the document it is about to read");
    assert!(error.to_string().contains("99"), "{error}");
}

#[test]
fn an_image_that_disagrees_with_the_contract_is_caught_by_the_image_half() {
    // Not reimplemented here: this is `Contract::verify_labels`, run from the cluster side, and
    // two implementations of it would be two chances to disagree about what a label means.
    let contract = contract();
    let metadata = stamped(&contract);

    let mut labels = image_labels(&contract);
    labels.insert(LABEL_PREFIX.to_owned(), "SOMETHINGELSE_".to_owned());

    let error = Pairing::new(&contract)
        .image(&labels, &image("portfolio", DIGEST_A))
        .object(metadata.labels(), metadata.annotations())
        .check()
        .expect_err("the running image reads a different namespace than the contract describes");
    assert!(error.to_string().contains(LABEL_PREFIX), "{error}");
}

#[test]
fn a_pairing_missing_a_half_says_which_half() {
    let contract = contract();
    let metadata = stamped(&contract);

    let error = Pairing::new(&contract)
        .object(metadata.labels(), metadata.annotations())
        .check()
        .expect_err("no image");
    assert!(error.to_string().contains("image"), "{error}");

    let error = Pairing::new(&contract)
        .image(&image_labels(&contract), &image("portfolio", DIGEST_A))
        .check()
        .expect_err("no object");
    assert!(error.to_string().contains("object"), "{error}");
}

#[test]
fn a_document_that_does_not_say_which_entry_it_is_fails_the_pairing() {
    let contract = contract();
    let metadata = stamped(&contract);

    for missing in [ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT] {
        let mut annotations = metadata.annotations().clone();
        annotations.remove(missing);
        let error = Pairing::new(&contract)
            .image(&image_labels(&contract), &image("portfolio", DIGEST_A))
            .object(metadata.labels(), &annotations)
            .check()
            .expect_err("a validator would have to guess");
        assert!(error.to_string().contains(missing), "{error}");
    }
}

// ---------------------------------------------------------------------------------------------
// The round trip, and the rendering
// ---------------------------------------------------------------------------------------------

#[test]
fn what_the_stamp_produces_is_what_the_check_accepts() {
    let contract = contract();

    for target in [document(), Target::Workload] {
        let metadata = contract
            .kube_metadata(&target, &[&image("portfolio", DIGEST_A)])
            .expect("a pinned reference");
        contract
            .verify_kube_metadata(&target, metadata.labels(), metadata.annotations())
            .expect("the metadata this contract produced is the metadata it accepts");
    }
}

#[test]
fn the_yaml_block_is_byte_stable_and_nests_at_whatever_it_is_given() {
    let contract = contract();
    let metadata = stamped(&contract);

    // Committed and diffed in review, so two calls that differ would show up as a change nobody
    // made. This is the `BTreeMap`-not-`HashMap` property, asserted rather than assumed.
    assert_eq!(metadata.to_yaml(2), metadata.to_yaml(2));

    for indent in [0, 2, 8] {
        let rendered = metadata.to_yaml(indent);
        let pad = " ".repeat(indent);

        assert!(rendered.ends_with('\n'), "indent {indent}: {rendered}");
        assert!(
            rendered.starts_with(&format!("{pad}labels:\n")),
            "indent {indent}: {rendered}"
        );
        assert!(
            rendered.contains(&format!("\n{pad}annotations:\n")),
            "indent {indent}: {rendered}"
        );

        for line in rendered.lines() {
            let depth = line.len() - line.trim_start().len();
            // A header sits at `indent`, an entry two deeper. Anything else is a block that
            // would nest under the wrong parent once it is pasted.
            let expected = if line.trim_start().starts_with("labels:")
                || line.trim_start().starts_with("annotations:")
            {
                indent
            } else {
                indent + 2
            };
            assert_eq!(depth, expected, "indent {indent}, line `{line}`");
        }

        // The version has to survive as a string. Unquoted, YAML reads `1` as an integer and the
        // API server refuses a label value that is not a string — a `helm install` failure rather
        // than a rendering one, so it is found late.
        assert!(
            rendered.contains(&format!("{LABEL_CONTRACT_VERSION}: \"1\"")),
            "indent {indent}: {rendered}"
        );
    }
}

#[test]
fn an_unfamiliar_format_survives_a_round_trip_rather_than_poisoning_it() {
    // The fallback variant, from the caller's side: a chart rendering something this build has
    // never heard of still produces a stamp, and the stamp still verifies. A closed enum would
    // have made this a refusal, and the refusal would have been of a perfectly good deployment.
    let contract = contract();
    let target = Target::document("config.hcl", Format::Other("hcl".to_owned()));

    let metadata = contract
        .kube_metadata(&target, &[&image("portfolio", DIGEST_A)])
        .expect("an unfamiliar format is still a format");
    assert_eq!(metadata.annotations()[ANNOTATION_FORMAT], "hcl");

    contract
        .verify_kube_metadata(&target, metadata.labels(), metadata.annotations())
        .expect("round trips like any other");

    assert_eq!(
        Format::from_annotation("hcl"),
        Format::Other("hcl".to_owned())
    );
}

#[test]
fn a_document_key_that_is_not_a_file_name_is_refused() {
    let contract = contract();

    // `.` and `..` are what the character class admits and a volume mount cannot: a `ConfigMap`
    // key becomes a file name, and neither of those names a file.
    for illegal in [".", "..", "", "sub/config.toml", "config toml"] {
        let error = contract
            .kube_metadata(
                &Target::document(illegal, Format::Toml),
                &[&image("portfolio", DIGEST_A)],
            )
            .expect_err("not a legal ConfigMap data key");
        assert!(error.to_string().contains("ConfigMap"), "{error}");
    }
}
