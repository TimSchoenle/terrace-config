//! The Kubernetes half of the contract protocol, and the pairing that joins it to the image half.
//!
//! `contract.rs` checks the image side, where a wrong answer produces a label a pipeline cannot
//! find a document with. Here a wrong answer produces something worse in two different ways.
//!
//! The first is that **an illegal value is not a failed check, it is a failed deploy**. Kubernetes
//! refuses a label value at `kubectl apply`, so a stamp this crate got wrong does not weaken a
//! gate — it stops the chart rolling out at all, far from whatever decided the value, with a
//! message naming the object rather than the crate. Every value the module emits therefore has a
//! test that it is legal, and the contract those tests run against is one whose prefix and app name
//! are as hostile to the label rules as a real service's can be.
//!
//! The second is that a pairing which passes when it should not is a validator reporting that a
//! `ConfigMap` and an image agree when they were rendered a release apart. That is the whole
//! failure this feature exists to catch, so the negative cases here — the skew, the fourth image,
//! the unpinned reference — matter more than the positive one.

#![cfg(feature = "schema")]
#![expect(dead_code, reason = "fixtures are read by the derive, not at runtime")]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use terrace_config::Terrace;
use terrace_config::schema::kube::{
    ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT, ANNOTATION_IMAGES, Format, LABEL_CONTRACT_VERSION,
    Metadata, Pairing, Target,
};
use terrace_config::schema::{
    App, CONTRACT_VERSION, Contract, DEFAULT_PATH, Describe, LABEL_PREFIX, LABEL_VERSION, Schema,
};

#[derive(Deserialize, Serialize, Default, Describe)]
struct Config {
    /// Bundle directory the readiness probe checks.
    #[serde(default = "default_dist")]
    dist_dir: String,
    /// Revalidation interval in seconds.
    #[serde(default)]
    ttl_secs: u64,
}

fn default_dist() -> String {
    "public".to_owned()
}

/// The loader every test here describes against.
///
/// The prefix is the hostile part and it is not contrived: `PORTFOLIO_` is what the worked example
/// uses everywhere else in this repository, and a trailing underscore is illegal in a Kubernetes
/// label value. A dialect prefix always ends in a separator, so *every* contract this crate can
/// build has a prefix that cannot be a label value — which is the constraint the whole module is
/// shaped by, and the reason a test proving the stamp is legal is worth having at all.
fn terrace() -> Terrace {
    Terrace::new("PORTFOLIO_")
}

fn schema() -> Schema {
    terrace()
        .schema::<Config>()
        .with_defaults_from(&Config::default())
        .expect("the default config serialises")
}

/// A contract whose every interesting string is illegal as a Kubernetes label value.
///
/// The app name carries a `/` and a space, and the version a `+` — none of which any rule in this
/// crate forbids, and all of which a `kubectl apply` would reject if they reached a label. Nothing
/// here reaches one, which is the assertion.
fn contract() -> Contract {
    schema()
        .into_contract(App::new("portfolio/web service").version("v2.5.0+build.7"))
        .build()
        .expect("the contract has nothing to refuse")
}

/// One digest-pinned reference, and two more that differ only in their digest.
const PORTFOLIO: &str =
    "ghcr.io/you/portfolio@sha256:48e259cb4d7c1f0a2b3e5d6c7a8b9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d";
const SIDECAR: &str =
    "ghcr.io/you/sidecar@sha256:9f1c0d2e3b4a5968778695a4b3c2d1e0f9e8d7c6b5a4938271605f4e3d2c1b0a";
const MIGRATOR: &str =
    "ghcr.io/you/migrator@sha256:0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";
/// An image that reads a different document entirely.
const STRANGER: &str =
    "ghcr.io/you/unrelated@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

/// The image labels a correctly built image carries, as `crane config` reports them.
fn image_labels(contract: &Contract) -> BTreeMap<String, String> {
    contract
        .labels(DEFAULT_PATH)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

fn document() -> Target {
    Target::document("config.toml", Format::Toml)
}

// ---------------------------------------------------------------------------------------------
// Everything emitted is something Kubernetes will accept
// ---------------------------------------------------------------------------------------------

/// The rule Kubernetes applies to a label value, restated here rather than called.
///
/// The crate's own predicate is what is under test, so asking it whether its output is legal would
/// agree with itself by construction. This is the second opinion.
fn is_legal_label_value(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    if value.len() > 63 {
        return false;
    }
    let bytes = value.as_bytes();
    bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// The rule Kubernetes applies to a label or annotation key, restated for the same reason.
fn is_legal_key(key: &str) -> bool {
    let (prefix, name) = match key.split_once('/') {
        Some((prefix, name)) => (Some(prefix), name),
        None => (None, key),
    };
    if let Some(prefix) = prefix {
        if prefix.is_empty() || prefix.len() > 253 {
            return false;
        }
        let legal_subdomain = prefix.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && label
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphanumeric())
                && label
                    .bytes()
                    .next_back()
                    .is_some_and(|b| b.is_ascii_alphanumeric())
        });
        if !legal_subdomain {
            return false;
        }
    }
    !name.is_empty() && is_legal_label_value(name)
}

#[test]
fn every_key_and_value_a_stamp_carries_is_one_kubernetes_will_accept() {
    let contract = contract();
    // The prefix the image publishes is illegal as a label value, which is the premise. If this
    // ever stops being true the test below stops proving anything.
    assert!(!is_legal_label_value("PORTFOLIO_"));

    for target in [document(), Target::Workload] {
        let metadata = contract
            .kube_metadata(&target, &[PORTFOLIO, SIDECAR])
            .expect("a stamp for a contract the builder accepted");

        for (key, value) in metadata.labels() {
            assert!(is_legal_key(key), "`{key}` is not a legal label key");
            assert!(
                is_legal_label_value(value),
                "`{key}` carries `{value}`, which is not a legal label value"
            );
        }
        // Only the keys are constrained on an annotation. The values are not, which is exactly
        // why the image list and the prefix-shaped values live here.
        for key in metadata.annotations().keys() {
            assert!(is_legal_key(key), "`{key}` is not a legal annotation key");
        }
    }
}

#[test]
fn the_only_label_is_the_one_a_selector_matches_on() {
    let metadata = contract()
        .kube_metadata(&document(), &[PORTFOLIO])
        .expect("stamped");

    assert_eq!(metadata.labels().len(), 1);
    assert_eq!(
        metadata
            .labels()
            .get(LABEL_CONTRACT_VERSION)
            .map(String::as_str),
        Some(CONTRACT_VERSION.to_string().as_str())
    );
    // Neither of the two facts about an image that the image already carries.
    assert!(!metadata.labels().contains_key(LABEL_PREFIX));
    assert!(
        metadata
            .labels()
            .keys()
            .all(|key| !key.contains("contract-path") && !key.contains("app"))
    );
}

// ---------------------------------------------------------------------------------------------
// The two targets
// ---------------------------------------------------------------------------------------------

#[test]
fn a_workload_stamp_carries_the_image_list_and_nothing_about_a_document() {
    let metadata = contract()
        .kube_metadata(&Target::Workload, &[PORTFOLIO])
        .expect("stamped");

    // The label, so a policy selects it, and the image list, so an admission webhook holding only
    // a pod can find the images without walking ownership references.
    assert!(metadata.labels().contains_key(LABEL_CONTRACT_VERSION));
    assert!(metadata.annotations().contains_key(ANNOTATION_IMAGES));
    // A pod is not a document.
    assert!(!metadata.annotations().contains_key(ANNOTATION_DOCUMENT_KEY));
    assert!(!metadata.annotations().contains_key(ANNOTATION_FORMAT));
}

#[test]
fn a_document_stamp_names_the_entry_and_the_parser() {
    let metadata = contract()
        .kube_metadata(&Target::document("app.yaml", Format::Yaml), &[PORTFOLIO])
        .expect("stamped");

    assert_eq!(
        metadata
            .annotations()
            .get(ANNOTATION_DOCUMENT_KEY)
            .map(String::as_str),
        Some("app.yaml")
    );
    assert_eq!(
        metadata
            .annotations()
            .get(ANNOTATION_FORMAT)
            .map(String::as_str),
        Some("yaml")
    );
}

#[test]
fn a_document_block_copied_onto_a_pod_template_is_refused() {
    let contract = contract();
    let document = contract
        .kube_metadata(&document(), &[PORTFOLIO])
        .expect("stamped");

    // Every key is one this crate wrote, and every value is correct for a document. What is wrong
    // is the object it landed on, and nothing but this check can see that.
    let error = contract
        .verify_kube_metadata(&Target::Workload, document.labels(), document.annotations())
        .expect_err("a pod is not a document");
    assert!(
        error.to_string().contains(ANNOTATION_DOCUMENT_KEY),
        "{error}"
    );
}

// ---------------------------------------------------------------------------------------------
// Pinning
// ---------------------------------------------------------------------------------------------

#[test]
fn an_image_reference_that_is_not_digest_pinned_is_refused() {
    let contract = contract();

    for unpinned in [
        "ghcr.io/you/portfolio",
        "ghcr.io/you/portfolio:v2.5.0",
        "ghcr.io/you/portfolio@sha256:48e259cb",
        "ghcr.io/you/portfolio@sha512:48e259cb4d7c1f0a2b3e5d6c7a8b9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d",
    ] {
        let error = contract
            .kube_metadata(&document(), &[unpinned])
            .expect_err("an unpinned reference proves nothing");
        assert!(error.to_string().contains(unpinned), "{error}");
    }
}

#[test]
fn a_stamp_for_no_images_at_all_is_refused() {
    let error = contract()
        .kube_metadata(&document(), &[])
        .expect_err("a document nothing reads");
    assert!(error.to_string().contains(ANNOTATION_IMAGES), "{error}");
}

#[test]
fn one_image_named_twice_is_refused() {
    let error = contract()
        .kube_metadata(&document(), &[PORTFOLIO, SIDECAR, PORTFOLIO])
        .expect_err("a repeat is a template that meant to name two");
    assert!(error.to_string().contains(PORTFOLIO), "{error}");
}

// ---------------------------------------------------------------------------------------------
// The pairing
// ---------------------------------------------------------------------------------------------

#[test]
fn a_stamp_this_contract_produced_is_one_it_pairs_with_its_own_image() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[PORTFOLIO])
        .expect("stamped");

    Pairing::new(&contract, DEFAULT_PATH)
        .image(PORTFOLIO, &image_labels(&contract))
        .object(metadata.labels(), metadata.annotations())
        .check()
        .expect("the two halves this contract produced describe one surface");
}

#[test]
fn every_image_in_a_union_pairs_with_the_one_document_they_share() {
    let contract = contract();
    let labels = image_labels(&contract);
    // The eight-binary case from the plan, cut to three: one document, one prefix, several images
    // whose `Describe` each covers only the keys it consumes.
    let metadata = contract
        .kube_metadata(&document(), &[PORTFOLIO, SIDECAR, MIGRATOR])
        .expect("stamped");

    for running in [PORTFOLIO, SIDECAR, MIGRATOR] {
        Pairing::new(&contract, DEFAULT_PATH)
            .image(running, &labels)
            .object(metadata.labels(), metadata.annotations())
            .check()
            .unwrap_or_else(|error| panic!("`{running}` reads this document: {error}"));
    }

    // Membership, not "any image with a valid stamp". A fourth image mounting this document is
    // either a pod mounting the wrong `ConfigMap` or a chart that pinned a digest without
    // re-rendering, and both are the failure this exists for.
    let error = Pairing::new(&contract, DEFAULT_PATH)
        .image(STRANGER, &labels)
        .object(metadata.labels(), metadata.annotations())
        .check()
        .expect_err("a fourth image is not in the union");
    assert!(error.to_string().contains(STRANGER), "{error}");
}

#[test]
fn a_version_skew_between_the_object_and_the_image_names_both_sides() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[PORTFOLIO])
        .expect("stamped");

    // The object was rendered by a chart still on the previous protocol version, and the image was
    // built from this one. Neither side is internally wrong, which is why only a pairing sees it.
    let mut stale = metadata.labels().clone();
    stale.insert(LABEL_CONTRACT_VERSION.to_owned(), "0".to_owned());

    let error = Pairing::new(&contract, DEFAULT_PATH)
        .image(PORTFOLIO, &image_labels(&contract))
        .object(&stale, metadata.annotations())
        .check()
        .expect_err("a skew is refused");
    let message = error.to_string();
    assert!(message.contains('0'), "{message}");
    assert!(message.contains(&CONTRACT_VERSION.to_string()), "{message}");
    assert!(message.contains(LABEL_CONTRACT_VERSION), "{message}");
}

#[test]
fn an_image_whose_labels_disagree_with_the_contract_is_refused_by_the_pairing() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[PORTFOLIO])
        .expect("stamped");

    // The image half, checked by `Contract::verify_labels` rather than by a second implementation
    // of the same rule living here.
    let mut labels = image_labels(&contract);
    labels.insert(LABEL_PREFIX.to_owned(), "OTHER_".to_owned());

    let error = Pairing::new(&contract, DEFAULT_PATH)
        .image(PORTFOLIO, &labels)
        .object(metadata.labels(), metadata.annotations())
        .check()
        .expect_err("the image does not carry this contract");
    assert!(error.to_string().contains(LABEL_PREFIX), "{error}");
}

#[test]
fn an_image_carrying_no_contract_version_label_is_refused_by_the_pairing() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[PORTFOLIO])
        .expect("stamped");

    let mut labels = image_labels(&contract);
    labels.remove(LABEL_VERSION);

    let error = Pairing::new(&contract, DEFAULT_PATH)
        .image(PORTFOLIO, &labels)
        .object(metadata.labels(), metadata.annotations())
        .check()
        .expect_err("an image that declares nothing cannot be shown to agree");
    assert!(error.to_string().contains(LABEL_VERSION), "{error}");
}

#[test]
fn half_a_question_is_refused_rather_than_answered() {
    let contract = contract();

    let error = Pairing::new(&contract, DEFAULT_PATH)
        .check()
        .expect_err("nothing to compare");
    assert!(error.to_string().contains("Pairing::image"), "{error}");
}

// ---------------------------------------------------------------------------------------------
// What is ignored, and what is not
// ---------------------------------------------------------------------------------------------

#[test]
fn the_labels_and_annotations_a_chart_adds_of_its_own_are_ignored() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[PORTFOLIO])
        .expect("stamped");

    // What every chart in the world puts on every object it renders. None of it is this document's
    // business, exactly as `verify_labels` ignores an image's `org.opencontainers.image.*`.
    let mut labels = metadata.labels().clone();
    labels.insert("app.kubernetes.io/name".to_owned(), "portfolio".to_owned());
    labels.insert("app.kubernetes.io/version".to_owned(), "2.5.0".to_owned());
    labels.insert("helm.sh/chart".to_owned(), "portfolio-1.4.2".to_owned());

    let mut annotations = metadata.annotations().clone();
    annotations.insert(
        "kubectl.kubernetes.io/last-applied-configuration".to_owned(),
        "{}".to_owned(),
    );

    contract
        .verify_kube_metadata(&document(), &labels, &annotations)
        .expect("a chart's own labels are not this crate's business");

    Pairing::new(&contract, DEFAULT_PATH)
        .image(PORTFOLIO, &image_labels(&contract))
        .object(&labels, &annotations)
        .check()
        .expect("nor do they change the pairing");
}

#[test]
fn a_stamp_round_trips_through_the_check_that_reads_it() {
    let contract = contract();

    for target in [
        document(),
        Target::document("logging.json", Format::Json),
        Target::document("x.conf", Format::Other("ini".to_owned())),
        Target::Workload,
    ] {
        let metadata = contract
            .kube_metadata(&target, &[PORTFOLIO, SIDECAR])
            .expect("stamped");
        contract
            .verify_kube_metadata(&target, metadata.labels(), metadata.annotations())
            .unwrap_or_else(|error| panic!("what it wrote, it must accept: {error}"));
    }
}

#[test]
fn a_document_key_the_object_disagrees_about_is_refused() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[PORTFOLIO])
        .expect("stamped");

    let mut annotations = metadata.annotations().clone();
    annotations.insert(ANNOTATION_DOCUMENT_KEY.to_owned(), "other.toml".to_owned());

    let error = contract
        .verify_kube_metadata(&document(), metadata.labels(), &annotations)
        .expect_err("a validator would read the wrong entry out of `data`");
    assert!(error.to_string().contains("other.toml"), "{error}");
    assert!(error.to_string().contains("config.toml"), "{error}");
}

// ---------------------------------------------------------------------------------------------
// The block a chart pastes
// ---------------------------------------------------------------------------------------------

fn yaml_of(indent: usize) -> String {
    contract()
        .kube_metadata(&document(), &[PORTFOLIO, SIDECAR])
        .expect("stamped")
        .to_yaml(indent)
}

#[test]
fn the_pasted_block_is_byte_stable() {
    // The same reason `Contract::to_json` is byte-stable: a chart commits this, and a rendering
    // that reordered itself between two runs would be a diff in every pull request that touched
    // anything nearby.
    assert_eq!(yaml_of(2), yaml_of(2));

    let metadata: Metadata = contract()
        .kube_metadata(&document(), &[PORTFOLIO, SIDECAR])
        .expect("stamped");
    assert_eq!(metadata.to_yaml(2), metadata.to_yaml(2));
}

#[test]
fn the_pasted_block_indents_where_it_was_asked_to() {
    for indent in [0, 2, 8] {
        let rendered = yaml_of(indent);
        let outer = " ".repeat(indent);
        let inner = " ".repeat(indent + 2);

        assert!(
            rendered.ends_with('\n'),
            "indent {indent}: no trailing newline"
        );
        assert!(
            !rendered.contains('\t'),
            "indent {indent}: a tab is not YAML indentation"
        );

        let mut blocks = 0;
        for line in rendered.lines() {
            assert!(!line.is_empty(), "indent {indent}: a blank line");
            if line.ends_with(':') {
                // A block key: `labels:` or `annotations:`, at exactly `indent`.
                assert_eq!(
                    line,
                    format!("{outer}{}", line.trim_start()),
                    "indent {indent}: a block key is not at column {indent}"
                );
                blocks += 1;
            } else {
                // An entry, two deeper, and quoted so that `"1"` stays a string rather than
                // becoming the integer 1 — which is not a thing a label value may be.
                assert!(
                    line.starts_with(&inner) && !line.starts_with(&format!("{inner} ")),
                    "indent {indent}: `{line}` is not at column {}",
                    indent + 2
                );
                let (key, value) = line.trim_start().split_once(": ").expect("`key: value`");
                assert!(is_legal_key(key), "indent {indent}: `{key}`");
                assert!(
                    value.starts_with('"') && value.ends_with('"'),
                    "indent {indent}: `{value}` is unquoted"
                );
            }
        }
        assert_eq!(blocks, 2, "indent {indent}: labels and annotations");
    }
}

#[test]
fn the_pasted_block_is_the_metadata_and_nothing_else() {
    let rendered = yaml_of(2);

    // Spelled out once, so that a change to the rendering is a change to a test somebody reads
    // rather than a predicate that quietly still passes.
    assert_eq!(
        rendered,
        concat!(
            "  labels:\n",
            "    dev.terrace.config/contract-version: \"1\"\n",
            "  annotations:\n",
            "    dev.terrace.config/document-key: \"config.toml\"\n",
            "    dev.terrace.config/format: \"toml\"\n",
            "    dev.terrace.config/images: \"",
            "ghcr.io/you/portfolio@sha256:48e259cb4d7c1f0a2b3e5d6c7a8b9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d",
            ",",
            "ghcr.io/you/sidecar@sha256:9f1c0d2e3b4a5968778695a4b3c2d1e0f9e8d7c6b5a4938271605f4e3d2c1b0a",
            "\"\n",
        )
    );
}
