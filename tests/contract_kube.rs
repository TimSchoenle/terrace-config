//! The cluster-side stamp: what a rendered Kubernetes object carries, and what pairs it with the
//! image that reads it.
//!
//! `contract.rs` asserts about a document a pipeline reads. This file asserts about metadata an
//! **API server** accepts or refuses, which is a different kind of wrong answer: a document with a
//! bad field is caught by whoever parses it, whereas a label value the platform will not hold is
//! caught at `kubectl apply`, in a chart repository, by somebody who did not write it.
//!
//! So the character rules are restated here rather than reached through the crate's own
//! validators. A test that asked the code under test which values are legal would agree with it by
//! construction, and this is the one place whose job is to be able to disagree.

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
    App, CONTRACT_VERSION, Contract, DEFAULT_PATH, Describe, LABEL_PREFIX, LABEL_VERSION, Schema,
};

#[derive(Deserialize, Serialize, Default, Describe)]
struct Config {
    /// Bundle directory the readiness probe checks.
    #[serde(default)]
    dist_dir: String,
    #[config(nested)]
    github: Github,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Github {
    /// User whose repositories are listed.
    username: String,
}

/// Three digests that differ in their first characters, so a failure names which one it meant.
const DIGEST_A: &str = "sha256:48e259cb0e5f4b3a6d1c8f97a2b4e6d0c3a5f7192b4d6e8a0c2e4f6a8b0d2e4f";
const DIGEST_B: &str = "sha256:9f1c37ad5e0b2c4d6e8f0a1b3c5d7e9f0a2b4c6d8e0f1a3b5c7d9e1f3a5b7c9d";
const DIGEST_C: &str = "sha256:c0ffee11223344556677889900aabbccddeeff00112233445566778899aabbcd";

fn image_a() -> String {
    format!("ghcr.io/you/portfolio@{DIGEST_A}")
}

fn image_b() -> String {
    format!("ghcr.io/you/sidecar@{DIGEST_B}")
}

fn image_c() -> String {
    format!("ghcr.io/you/migrator@{DIGEST_C}")
}

fn schema() -> Schema {
    Terrace::new("PORTFOLIO_")
        .schema::<Config>()
        .with_defaults_from(&Config::default())
        .expect("the default config serialises")
}

fn contract() -> Contract {
    schema()
        .into_contract(App::new("portfolio").version("v2.5.0"))
        .build()
        .expect("the contract has nothing to refuse")
}

fn document() -> Target {
    Target::document("config.toml", Format::Toml)
}

/// The image's own `config.Labels`, as `crane config` would report them for a build that pasted
/// what `Contract::to_dockerfile_labels` emitted.
fn image_labels(contract: &Contract) -> BTreeMap<String, String> {
    contract
        .labels(DEFAULT_PATH)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The platform's own rules, restated
// ---------------------------------------------------------------------------------------------

/// Whether Kubernetes would hold `value` as a label value.
///
/// At most 63 characters, and unless empty it begins and ends with an alphanumeric character with
/// only `-`, `_` and `.` between. Written from the platform's rule rather than from the crate's, so
/// that the two can disagree.
fn is_label_value(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    if value.len() > 63 {
        return false;
    }
    let alphanumeric = |c: char| c.is_ascii_alphanumeric();
    value.starts_with(alphanumeric)
        && value.ends_with(alphanumeric)
        && value
            .chars()
            .all(|c| alphanumeric(c) || c == '-' || c == '_' || c == '.')
}

/// Whether Kubernetes would hold `key` as a label or annotation key.
fn is_metadata_key(key: &str) -> bool {
    let (prefix, name) = match key.split_once('/') {
        None => ("", key),
        Some((prefix, name)) => (prefix, name),
    };
    if name.contains('/') || name.is_empty() || !is_label_value(name) {
        return false;
    }
    if prefix.is_empty() {
        return !key.contains('/');
    }
    prefix.len() <= 253
        && prefix.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !part.starts_with('-')
                && !part.ends_with('-')
        })
}

/// Assert that every entry of a stamp is one the API server would accept.
fn assert_kubernetes_legal(metadata: &Metadata) {
    for (key, value) in metadata.labels() {
        assert!(is_metadata_key(key), "`{key}` is not a label key");
        assert!(
            is_label_value(value),
            "`{key}` carries `{value}`, which is not a label value"
        );
    }
    // Annotation *values* are unconstrained by the platform — only the keys are — so only the keys
    // are asserted about here. That asymmetry is the whole reason the image list is an annotation.
    for key in metadata.annotations().keys() {
        assert!(is_metadata_key(key), "`{key}` is not an annotation key");
    }
}

// ---------------------------------------------------------------------------------------------
// What the stamp carries
// ---------------------------------------------------------------------------------------------

#[test]
fn a_stamp_is_legal_even_when_the_contract_it_describes_is_hostile_to_the_label_rules() {
    // A prefix that is a label value in none of the three ways it could fail — it is longer than
    // 63 characters, it ends in `_`, and it is upper case — beside an app name carrying `/`, `+`
    // and a space. Both are perfectly legal in a contract, and neither reaches the stamp: this is
    // the test that says the omissions argued in `kube.rs` are omissions rather than intentions.
    let hostile_prefix = "A_SERVICE_PREFIX_CONSIDERABLY_LONGER_THAN_SIXTY_THREE_CHARACTERS_LONG_";
    let contract = Terrace::new(hostile_prefix)
        .schema::<Config>()
        .with_defaults_from(&Config::default())
        .expect("the default config serialises")
        .into_contract(App::new("not/a label+value").version("v2.5.0"))
        .build()
        .expect("neither a long prefix nor an odd app name is a contract's business");

    let metadata = contract
        .kube_metadata(&document(), &[&image_a()])
        .expect("the stamp carries neither the prefix nor the app name");
    assert_kubernetes_legal(&metadata);

    let rendered = format!("{:?}", metadata.labels()) + &format!("{:?}", metadata.annotations());
    assert!(
        !rendered.contains(hostile_prefix) && !rendered.contains("not/a label+value"),
        "the stamp leaked a value the image already publishes: {rendered}"
    );

    // And the workload half, which carries a strict subset.
    assert_kubernetes_legal(
        &contract
            .kube_metadata(&Target::Workload, &[&image_a()])
            .expect("a workload stamp is the label and the image list"),
    );
}

#[test]
fn the_document_stamp_names_every_entry_a_validator_needs() {
    let metadata = contract()
        .kube_metadata(&document(), &[&image_a()])
        .expect("stamped");

    assert_eq!(
        metadata.labels(),
        &BTreeMap::from([(
            LABEL_CONTRACT_VERSION.to_owned(),
            CONTRACT_VERSION.to_string()
        )])
    );
    assert_eq!(
        metadata.annotations(),
        &BTreeMap::from([
            (ANNOTATION_IMAGES.to_owned(), image_a()),
            (ANNOTATION_DOCUMENT_KEY.to_owned(), "config.toml".to_owned()),
            (ANNOTATION_FORMAT.to_owned(), "toml".to_owned()),
        ])
    );

    // Every key belongs to this protocol, so a chart merging the stamp into its own metadata can
    // never be told it clobbered something.
    for key in metadata
        .labels()
        .keys()
        .chain(metadata.annotations().keys())
    {
        assert!(
            key.starts_with(NAMESPACE),
            "`{key}` is outside the namespace"
        );
    }
}

#[test]
fn a_workload_carries_the_image_list_and_nothing_a_document_would() {
    // A pod is not a document. The image list is there so that an admission webhook seeing only
    // the pod — which is what an admission webhook usually sees — can find it without walking
    // ownership references to an object that may not exist yet.
    let metadata = contract()
        .kube_metadata(&Target::Workload, &[&image_a()])
        .expect("stamped");

    assert_eq!(metadata.labels().len(), 1);
    assert_eq!(
        metadata.annotations(),
        &BTreeMap::from([(ANNOTATION_IMAGES.to_owned(), image_a())])
    );
    assert!(!metadata.annotations().contains_key(ANNOTATION_DOCUMENT_KEY));
    assert!(!metadata.annotations().contains_key(ANNOTATION_FORMAT));
}

#[test]
fn a_workload_that_claims_a_document_key_is_refused_rather_than_ignored() {
    // Extra keys are ignored; this one is not extra. It is this protocol's own key on an object
    // that has no document, so its value is a claim nothing checked against the file the pod
    // actually mounts — the second spelling the whole design refuses.
    let contract = contract();
    let mut annotations = BTreeMap::from([(ANNOTATION_IMAGES.to_owned(), image_a())]);
    annotations.insert(ANNOTATION_DOCUMENT_KEY.to_owned(), "config.toml".to_owned());

    let labels = BTreeMap::from([(
        LABEL_CONTRACT_VERSION.to_owned(),
        CONTRACT_VERSION.to_string(),
    )]);
    let error = contract
        .verify_kube_metadata(&Target::Workload, &labels, &annotations)
        .expect_err("refused");
    assert!(
        error.to_string().contains(ANNOTATION_DOCUMENT_KEY),
        "{error}"
    );
}

#[test]
fn an_unpinned_reference_is_refused_because_a_tag_can_be_moved() {
    let contract = contract();

    let error = contract
        .kube_metadata(&document(), &["ghcr.io/you/portfolio:v2.5.0"])
        .expect_err("refused");
    assert!(error.to_string().contains("moved"), "{error}");

    // The digest has to be this algorithm, this length, and lower case: a reference that only
    // looks pinned is worse than one that plainly is not, because it passes a reader's eye.
    for reference in [
        "ghcr.io/you/portfolio",
        "ghcr.io/you/portfolio@sha256:deadbeef",
        "ghcr.io/you/portfolio@sha512:0123456789abcdef",
        &format!("ghcr.io/you/portfolio@{}", DIGEST_A.to_uppercase()),
        &format!("@{DIGEST_A}"),
    ] {
        assert!(
            contract.kube_metadata(&document(), &[reference]).is_err(),
            "`{reference}` was accepted as a digest-pinned reference"
        );
    }

    // An empty list is refused for the same reason an unpinned one is: neither pairs the object
    // with anything, and an object no policy can decide about is worse than no stamp at all.
    assert!(contract.kube_metadata(&document(), &[]).is_err());
    // A repeated member says nothing the first one did not.
    assert!(
        contract
            .kube_metadata(&document(), &[&image_a(), &image_a()])
            .is_err()
    );
}

// ---------------------------------------------------------------------------------------------
// The pairing
// ---------------------------------------------------------------------------------------------

#[test]
fn a_stamp_this_crate_produced_is_one_it_accepts() {
    let contract = contract();
    for target in [document(), Target::Workload] {
        let metadata = contract
            .kube_metadata(&target, &[&image_a(), &image_b()])
            .expect("stamped");
        contract
            .verify_kube_metadata(&target, metadata.labels(), metadata.annotations())
            .unwrap_or_else(|error| panic!("{target:?} did not round-trip: {error}"));
    }
}

#[test]
fn the_metadata_a_chart_adds_of_its_own_is_none_of_this_check_s_business() {
    // The same rule `Contract::verify_labels` applies to an image's `org.opencontainers.image.*`:
    // an object carries a great deal that this protocol did not put there, and refusing on it
    // would make the check a reason not to adopt the protocol.
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[&image_a()])
        .expect("stamped");

    let mut labels = metadata.labels().clone();
    labels.insert("app.kubernetes.io/name".to_owned(), "portfolio".to_owned());
    labels.insert("app.kubernetes.io/managed-by".to_owned(), "Helm".to_owned());
    let mut annotations = metadata.annotations().clone();
    annotations.insert(
        "kubectl.kubernetes.io/last-applied-configuration".to_owned(),
        "{}".to_owned(),
    );
    annotations.insert("checksum/config".to_owned(), "a1b2c3".to_owned());

    contract
        .verify_kube_metadata(&document(), &labels, &annotations)
        .expect("what the chart adds of its own is ignored");
}

#[test]
fn one_document_read_by_three_images_pairs_with_each_of_them() {
    // The union case. A `tankovault`-shaped chart renders one document that several binaries read,
    // so the annotation is a list and the membership test is `contains`, not `==`. Equality here
    // would refuse every deployment the list exists for.
    let contract = contract();
    let images = [image_a(), image_b(), image_c()];
    let borrowed: Vec<&str> = images.iter().map(String::as_str).collect();
    let metadata = contract
        .kube_metadata(&document(), &borrowed)
        .expect("stamped");
    let labels = image_labels(&contract);

    for running in &images {
        Pairing::new(&contract)
            .image(running, DEFAULT_PATH, &labels)
            .object(&document(), metadata.labels(), metadata.annotations())
            .check()
            .unwrap_or_else(|error| panic!("`{running}` reads this document: {error}"));
    }

    // A fourth image, correctly labelled and correctly pinned, and still not one of the images
    // this document was rendered for.
    let stranger = format!("ghcr.io/you/stranger@{DIGEST_A}");
    let error = Pairing::new(&contract)
        .image(&stranger, DEFAULT_PATH, &labels)
        .object(&document(), metadata.labels(), metadata.annotations())
        .check()
        .expect_err("refused");
    let message = error.to_string();
    assert!(message.contains(&stranger), "{message}");
    assert!(message.contains(&image_b()), "{message}");
}

#[test]
fn a_version_skew_between_the_object_and_the_running_image_names_both_numbers() {
    // The failure the whole cluster-side half exists for: an image rolled forward onto a protocol
    // version the `ConfigMap` beside it was never re-rendered against. Whoever is holding the
    // alert has the object and the container, and the message has to name those two rather than
    // name a contract document they are not looking at.
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[&image_a()])
        .expect("stamped");

    let mut labels = image_labels(&contract);
    labels.insert(LABEL_VERSION.to_owned(), "2".to_owned());

    let error = Pairing::new(&contract)
        .image(&image_a(), DEFAULT_PATH, &labels)
        .object(&document(), metadata.labels(), metadata.annotations())
        .check()
        .expect_err("refused");
    let message = error.to_string();
    assert!(message.contains(LABEL_CONTRACT_VERSION), "{message}");
    assert!(message.contains(LABEL_VERSION), "{message}");
    assert!(message.contains('1') && message.contains('2'), "{message}");

    // And the other direction: an image carrying no contract labels at all cannot be paired with
    // an object that claims to participate.
    let error = Pairing::new(&contract)
        .image(&image_a(), DEFAULT_PATH, &BTreeMap::new())
        .object(&document(), metadata.labels(), metadata.annotations())
        .check()
        .expect_err("refused");
    assert!(error.to_string().contains(LABEL_VERSION), "{error}");
}

#[test]
fn the_image_half_of_the_pairing_is_the_check_that_already_existed() {
    // `Pairing` reuses `Contract::verify_labels` rather than restating it, which is what keeps the
    // two halves of the protocol from drifting. The evidence is that a label the *object* cannot
    // carry — the loader's prefix — is still checked, on the side that does carry it.
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[&image_a()])
        .expect("stamped");

    let mut labels = image_labels(&contract);
    labels.insert(LABEL_PREFIX.to_owned(), "OTHERSERVICE_".to_owned());

    let error = Pairing::new(&contract)
        .image(&image_a(), DEFAULT_PATH, &labels)
        .object(&document(), metadata.labels(), metadata.annotations())
        .check()
        .expect_err("refused");
    assert!(error.to_string().contains(LABEL_PREFIX), "{error}");
}

#[test]
fn a_pairing_missing_a_half_says_which_half() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[&image_a()])
        .expect("stamped");
    let labels = image_labels(&contract);

    let error = Pairing::new(&contract)
        .object(&document(), metadata.labels(), metadata.annotations())
        .check()
        .expect_err("refused");
    assert!(error.to_string().contains("image half"), "{error}");

    let error = Pairing::new(&contract)
        .image(&image_a(), DEFAULT_PATH, &labels)
        .check()
        .expect_err("refused");
    assert!(error.to_string().contains("object half"), "{error}");
}

#[test]
fn a_document_key_the_object_disagrees_about_is_refused() {
    // A `ConfigMap` may carry several files. An object saying `other.toml` while the check is
    // about `config.toml` would have a validator read one entry and check it against the
    // description of another — and report every key as unknown, confidently.
    let contract = contract();
    let metadata = contract
        .kube_metadata(&Target::document("other.toml", Format::Toml), &[&image_a()])
        .expect("stamped");

    let error = contract
        .verify_kube_metadata(&document(), metadata.labels(), metadata.annotations())
        .expect_err("refused");
    let message = error.to_string();
    assert!(
        message.contains("other.toml") && message.contains("config.toml"),
        "{message}"
    );

    // And the format, for the same reason one level down: the wrong parser reports every key as
    // missing rather than saying it could not read the file.
    let metadata = contract
        .kube_metadata(
            &Target::document("config.toml", Format::Yaml),
            &[&image_a()],
        )
        .expect("stamped");
    let error = contract
        .verify_kube_metadata(&document(), metadata.labels(), metadata.annotations())
        .expect_err("refused");
    assert!(error.to_string().contains("yaml"), "{error}");
}

#[test]
fn an_unfamiliar_format_is_carried_rather_than_refused() {
    // The fallback variant, end to end. A document this crate has no parser for is still a
    // document, and refusing to stamp it would make the protocol unusable by the next syntax
    // before that syntax exists.
    let contract = contract();
    let target = Target::document("config.hcl", Format::parse("hcl"));
    let metadata = contract
        .kube_metadata(&target, &[&image_a()])
        .expect("an unfamiliar format is still a token");
    assert_eq!(
        metadata
            .annotations()
            .get(ANNOTATION_FORMAT)
            .map(String::as_str),
        Some("hcl")
    );
    contract
        .verify_kube_metadata(&target, metadata.labels(), metadata.annotations())
        .expect("round-trips");
}

// ---------------------------------------------------------------------------------------------
// The block a chart pastes
// ---------------------------------------------------------------------------------------------

#[test]
fn the_pasted_block_is_byte_stable_and_indents_where_it_is_told_to() {
    let metadata = contract()
        .kube_metadata(&document(), &[&image_a(), &image_b()])
        .expect("stamped");

    // Byte-stable, which is what lets a rendered manifest be diffed: two runs of one chart that
    // produce two orderings produce a diff nobody can review.
    assert_eq!(metadata.to_yaml(2), metadata.to_yaml(2));

    for indent in [0, 2, 8] {
        let rendered = metadata.to_yaml(indent);
        assert!(
            rendered.ends_with('\n'),
            "indent {indent}: no trailing newline"
        );
        assert_indented_mapping(&rendered, indent);
    }

    // The shape a Helm template pastes, spelled out once so that a change to it is a change to
    // this line rather than to a chart nobody in this repository can see.
    assert_eq!(
        metadata.to_yaml(0),
        format!(
            "labels:\n  \"{LABEL_CONTRACT_VERSION}\": \"1\"\nannotations:\n  \
             \"{ANNOTATION_DOCUMENT_KEY}\": \"config.toml\"\n  \
             \"{ANNOTATION_FORMAT}\": \"toml\"\n  \
             \"{ANNOTATION_IMAGES}\": \"{}\"\n",
            [image_a(), image_b()].join(",")
        )
    );
}

/// Assert that `rendered` is a YAML mapping of mappings, at `indent` and `indent + 2`.
///
/// A hand-written check rather than a parser: pulling a YAML crate into `[dev-dependencies]` to
/// assert about six lines would be a dependency this repository would then have to argue for.
/// What matters is the property a parser would give — every heading at one column, every entry two
/// deeper, every key and value quoted — and that is checkable by looking.
fn assert_indented_mapping(rendered: &str, indent: usize) {
    let mut headings = 0;
    let mut entries = 0;
    for line in rendered.lines() {
        let depth = line.len() - line.trim_start_matches(' ').len();
        let body = line.trim_start_matches(' ');
        assert!(
            !body.is_empty(),
            "a blank line breaks the block:\n{rendered}"
        );

        if depth == indent {
            assert!(
                body == "labels:" || body == "annotations:",
                "`{body}` is not a heading this block emits:\n{rendered}"
            );
            headings += 1;
        } else {
            assert_eq!(
                depth,
                indent + 2,
                "`{body}` is at the wrong depth:\n{rendered}"
            );
            let (key, value) = body
                .split_once(": ")
                .unwrap_or_else(|| panic!("`{body}` is not a mapping entry:\n{rendered}"));
            // Both halves quoted. The key carries a `/`, and an unquoted `"1"` is the integer the
            // API server refuses.
            assert!(
                key.starts_with('"') && key.ends_with('"'),
                "`{key}` is unquoted:\n{rendered}"
            );
            assert!(
                value.starts_with('"') && value.ends_with('"'),
                "`{value}` is unquoted:\n{rendered}"
            );
            entries += 1;
        }
    }
    assert_eq!(headings, 2, "both mappings are present:\n{rendered}");
    assert_eq!(entries, 4, "every entry is present:\n{rendered}");
}
