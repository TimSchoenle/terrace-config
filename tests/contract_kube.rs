//! The Kubernetes half of the publication protocol: what a rendered object carries, and the
//! pairing that refuses an image and a document which do not belong together.
//!
//! `contract.rs` checks the image half against a Dockerfile. These tests check the half nothing
//! in this repository can check for itself, because the object is rendered by a chart in another
//! one — so every assertion here is either about a rule the *platform* enforces at `kubectl
//! apply` time, or about a disagreement that would otherwise be found by a pod that starts
//! cleanly on the wrong configuration.
//!
//! The character rules are re-implemented below rather than reached for through the crate. They
//! are the property under test; a test that asked the code under test whether it was right would
//! pass for any answer it gave.

#![cfg(feature = "schema")]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use terrace_config::Terrace;
use terrace_config::schema::kube::{
    ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT, ANNOTATION_IMAGES, Format, LABEL_CONTRACT_VERSION,
    Metadata, NAMESPACE, Pairing, Target,
};
use terrace_config::schema::{
    App, CONTRACT_VERSION, Contract, DEFAULT_PATH, Describe, LABEL_VERSION, Schema,
};

/// A configuration whose keys are ordinary. Nothing about the *keys* reaches a Kubernetes object;
/// what does is the envelope version, and this fixture is here so a real contract exists to
/// produce it.
#[derive(Deserialize, Serialize, Default, Describe)]
struct Config {
    /// Bundle directory the readiness probe checks.
    dist_dir: String,
}

/// A prefix and an app name chosen to be as hostile to the Kubernetes rules as the crate permits.
///
/// `PORTFOLIO_` ends in an underscore, which is illegal as a label value; the app name carries a
/// `/`, a `+`, a space and upper case, and is well past 63 characters. Neither may reach a key or
/// a value, and the reason neither does is that neither is *stamped* — see the omissions
/// documented on `LABEL_CONTRACT_VERSION`.
const HOSTILE_PREFIX: &str = "PORTFOLIO_";
const HOSTILE_APP: &str =
    "Portfolio/Web+App — a name no label value could ever carry, and long past sixty-three";

const DIGEST_A: &str = "sha256:48e259cb4c9d0e3f1a2b5c6d7e8f9012345678901234567890abcdefabcdef01";
const DIGEST_B: &str = "sha256:9f1c0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7";
const DIGEST_C: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn schema() -> Schema {
    Terrace::new(HOSTILE_PREFIX)
        .schema::<Config>()
        .with_defaults_from(&Config::default())
        .expect("the fixture serialises")
}

fn contract() -> Contract {
    schema()
        .into_contract(App::new(HOSTILE_APP).version("v2.5.0"))
        .build()
        .expect("the fixture is a contract this crate accepts")
}

fn image(digest: &str) -> String {
    format!("ghcr.io/you/portfolio@{digest}")
}

/// The image half, as `crane config` would report it.
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

// --- the platform's rules, restated ---------------------------------------------------------

/// Whether `value` is a legal Kubernetes label value.
///
/// Deliberately a second implementation of the rule the crate enforces. The claim under test is
/// "everything this module emits is legal", and asking the module is not a way to check it.
fn legal_label_value(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let bytes = value.as_bytes();
    value.len() <= 63
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Whether `key` is a legal Kubernetes label or annotation key.
fn legal_key(key: &str) -> bool {
    let (prefix, name) = match key.split_once('/') {
        Some((prefix, name)) => (Some(prefix), name),
        None => (None, key),
    };
    if let Some(prefix) = prefix {
        let labels: Vec<&str> = prefix.split('.').collect();
        let subdomain = !prefix.is_empty()
            && prefix.len() <= 253
            && labels.iter().all(|label| {
                let bytes = label.as_bytes();
                !label.is_empty()
                    && bytes
                        .iter()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
                    && bytes[0] != b'-'
                    && bytes[bytes.len() - 1] != b'-'
            });
        if !subdomain {
            return false;
        }
    }
    !name.is_empty() && name.len() <= 63 && legal_label_value(name)
}

// --- the tests ------------------------------------------------------------------------------

/// The property the whole module exists to hold: a contract whose own strings are illegal as
/// Kubernetes label values still produces a stamp Kubernetes accepts, because the illegal strings
/// are never stamped.
#[test]
fn nothing_hostile_in_a_contract_reaches_a_kubernetes_key_or_value() {
    let contract = contract();
    // The premise. If either of these ever became legal the test would still pass while proving
    // nothing, so the premise is asserted rather than assumed.
    assert!(!legal_label_value(HOSTILE_PREFIX));
    assert!(!legal_label_value(HOSTILE_APP));

    for target in [document(), Target::Workload] {
        let metadata = contract
            .kube_metadata(&target, &[&image(DIGEST_A)])
            .expect("a hostile contract still produces a legal stamp");

        for (key, value) in metadata.labels() {
            assert!(legal_key(key), "illegal label key `{key}`");
            assert!(legal_label_value(value), "illegal label value `{value}`");
        }
        // Annotation *values* are unconstrained — the images list is far past 63 characters on
        // purpose — so only the keys are held to the rule.
        for key in metadata.annotations().keys() {
            assert!(legal_key(key), "illegal annotation key `{key}`");
            assert!(key.starts_with(NAMESPACE), "`{key}` left the namespace");
        }

        let rendered = format!("{metadata:?}");
        assert!(!rendered.contains(HOSTILE_APP), "the app name was stamped");
        assert!(
            !rendered.contains(HOSTILE_PREFIX),
            "the prefix was stamped; it is a fact about the image and the image already carries it"
        );
    }
}

/// `document-key` and `format` describe a document. A pod is not one, and a pod template that
/// carried them would keep saying which key held the configuration after the `ConfigMap` behind it
/// changed — a stale claim nothing would ever contradict.
#[test]
fn a_workload_carries_the_image_list_and_nothing_that_describes_a_document() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&Target::Workload, &[&image(DIGEST_A)])
        .expect("a workload is stampable");

    assert_eq!(
        metadata
            .labels()
            .get(LABEL_CONTRACT_VERSION)
            .map(String::as_str),
        Some(CONTRACT_VERSION.to_string().as_str())
    );
    assert!(metadata.annotations().contains_key(ANNOTATION_IMAGES));
    assert!(!metadata.annotations().contains_key(ANNOTATION_DOCUMENT_KEY));
    assert!(!metadata.annotations().contains_key(ANNOTATION_FORMAT));

    // And the reverse: a pod template that grew a document's annotations by copy-paste is refused
    // rather than tolerated, because tolerating it is what lets the copy go stale unnoticed.
    let mut annotations = metadata.annotations().clone();
    annotations.insert(ANNOTATION_DOCUMENT_KEY.to_owned(), "config.toml".to_owned());
    let error = contract
        .verify_kube_metadata(&Target::Workload, metadata.labels(), &annotations)
        .expect_err("refused");
    assert!(
        error.to_string().contains(ANNOTATION_DOCUMENT_KEY),
        "{error}"
    );
}

/// A tag can be moved after the chart was rendered, so a pairing keyed on one proves nothing about
/// what is running. Refused where the value enters, not tolerated and checked leniently later.
#[test]
fn an_image_reference_a_tag_could_move_is_refused_at_both_ends() {
    let contract = contract();

    for reference in [
        "ghcr.io/you/portfolio:v2.5.0",
        "ghcr.io/you/portfolio",
        "ghcr.io/you/portfolio@sha256:48e2",
        "ghcr.io/you/portfolio@md5:48e259cb4c9d0e3f1a2b5c6d7e8f9012",
    ] {
        let error = contract
            .kube_metadata(&document(), &[reference])
            .expect_err("an unpinned reference is not evidence");
        assert!(error.to_string().contains(reference), "{error}");
    }

    // The same rule from the other side: an object a chart already applied is held to it too, so a
    // hand-edited annotation cannot buy what the generator refuses.
    let metadata = contract
        .kube_metadata(&document(), &[&image(DIGEST_A)])
        .expect("pinned");
    let mut annotations = metadata.annotations().clone();
    annotations.insert(
        ANNOTATION_IMAGES.to_owned(),
        "ghcr.io/you/portfolio:latest".to_owned(),
    );
    let error = contract
        .verify_kube_metadata(&document(), metadata.labels(), &annotations)
        .expect_err("refused");
    assert!(error.to_string().contains("digest-pinned"), "{error}");
}

/// The union case, which is why membership rather than equality: one rendered document read by
/// three binaries, each of which is a legitimate pod to find it mounted into — and a fourth image
/// that was never listed, which is a pod mounting configuration nobody rendered for it.
#[test]
fn a_document_read_by_three_images_pairs_with_each_of_them_and_with_no_fourth() {
    let contract = contract();
    let images = [image(DIGEST_A), image(DIGEST_B), image(DIGEST_C)];
    let references: Vec<&str> = images.iter().map(String::as_str).collect();

    let metadata = contract
        .kube_metadata(&document(), &references)
        .expect("three images are one document's readers");
    let labels = image_labels(&contract);

    for reference in &references {
        Pairing::new(&contract, DEFAULT_PATH)
            .image(reference, &labels)
            .object(&document(), metadata.labels(), metadata.annotations())
            .check()
            .unwrap_or_else(|error| panic!("`{reference}` is listed and must pair: {error}"));
    }

    let stranger = format!("ghcr.io/you/stranger@{DIGEST_A}");
    let error = Pairing::new(&contract, DEFAULT_PATH)
        .image(&stranger, &labels)
        .object(&document(), metadata.labels(), metadata.annotations())
        .check()
        .expect_err("an image nobody listed must not pair");
    assert!(error.to_string().contains(&stranger), "{error}");
    assert!(error.to_string().contains(ANNOTATION_IMAGES), "{error}");
}

/// The two spellings of one image that a chart and a running pod respectively produce. A check
/// that insisted on the exact string would be one nothing in a cluster could pass.
#[test]
fn a_tag_beside_the_digest_is_the_same_image_as_the_digest_alone() {
    let contract = contract();
    let listed = format!("ghcr.io/you/portfolio:v2.5.0@{DIGEST_A}");
    let metadata = contract
        .kube_metadata(&document(), &[&listed])
        .expect("pinned, and tagged as well");

    Pairing::new(&contract, DEFAULT_PATH)
        .image(&image(DIGEST_A), &image_labels(&contract))
        .object(&document(), metadata.labels(), metadata.annotations())
        .check()
        .expect("one image, two spellings");
}

/// A chart and a build a generation apart. Naming only one side would send an operator to the
/// wrong repository, which is why this check exists separately from the two that compare each
/// side against the contract.
#[test]
fn a_version_skew_between_the_object_and_the_image_names_both_sides() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[&image(DIGEST_A)])
        .expect("stamped");

    let mut labels = metadata.labels().clone();
    labels.insert(LABEL_CONTRACT_VERSION.to_owned(), "2".to_owned());

    let error = Pairing::new(&contract, DEFAULT_PATH)
        .image(&image(DIGEST_A), &image_labels(&contract))
        .object(&document(), &labels, metadata.annotations())
        .check()
        .expect_err("refused");

    let message = error.to_string();
    assert!(message.contains(LABEL_CONTRACT_VERSION), "{message}");
    assert!(message.contains(LABEL_VERSION), "{message}");
    assert!(message.contains('2'), "the object's version: {message}");
    assert!(
        message.contains(&CONTRACT_VERSION.to_string()),
        "the image's version: {message}"
    );
}

/// The image half is `Contract::verify_labels`, reused rather than copied — so an image whose
/// labels drifted fails the pairing with the message that function already writes.
#[test]
fn an_image_whose_own_labels_drifted_fails_the_pairing_before_the_object_is_read() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[&image(DIGEST_A)])
        .expect("stamped");

    let mut labels = image_labels(&contract);
    labels.insert(
        "dev.terrace.config.prefix".to_owned(),
        "SOMETHINGELSE_".to_owned(),
    );

    let error = Pairing::new(&contract, DEFAULT_PATH)
        .image(&image(DIGEST_A), &labels)
        .object(&document(), metadata.labels(), metadata.annotations())
        .check()
        .expect_err("refused");
    assert!(
        error.to_string().contains("dev.terrace.config.prefix"),
        "{error}"
    );
}

/// An object carries whatever its chart's conventions add, and none of that is this document's
/// business — the same rule `verify_labels` already applies to an image's `org.opencontainers.*`.
#[test]
fn labels_and_annotations_this_protocol_did_not_write_are_ignored() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[&image(DIGEST_A)])
        .expect("stamped");

    let mut labels = metadata.labels().clone();
    labels.insert("app.kubernetes.io/name".to_owned(), "portfolio".to_owned());
    labels.insert(
        "app.kubernetes.io/component".to_owned(),
        "server".to_owned(),
    );
    let mut annotations = metadata.annotations().clone();
    annotations.insert(
        "kubectl.kubernetes.io/last-applied-configuration".to_owned(),
        "{}".to_owned(),
    );

    contract
        .verify_kube_metadata(&document(), &labels, &annotations)
        .expect("a chart's own conventions are not this protocol's business");

    Pairing::new(&contract, DEFAULT_PATH)
        .image(&image(DIGEST_A), &image_labels(&contract))
        .object(&document(), &labels, &annotations)
        .check()
        .expect("still one configuration surface");
}

#[test]
fn what_the_generator_produced_is_what_the_verifier_accepts() {
    let contract = contract();
    for target in [document(), Target::Workload] {
        let metadata = contract
            .kube_metadata(&target, &[&image(DIGEST_A), &image(DIGEST_B)])
            .expect("stamped");
        contract
            .verify_kube_metadata(&target, metadata.labels(), metadata.annotations())
            .expect("a round trip through this module's own output must hold");
    }
}

/// The message a validator gets when the annotation points at a different file from the one it was
/// asked to check. Without this it follows the annotation, reads a file nobody meant, and passes.
#[test]
fn a_document_key_naming_another_file_is_a_disagreement_rather_than_a_preference() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(
            &Target::document("server.toml", Format::Toml),
            &[&image(DIGEST_A)],
        )
        .expect("stamped");

    let error = contract
        .verify_kube_metadata(&document(), metadata.labels(), metadata.annotations())
        .expect_err("refused");
    let message = error.to_string();
    assert!(message.contains("server.toml"), "{message}");
    assert!(message.contains("config.toml"), "{message}");
}

/// A format this version has never heard of is carried rather than refused — the fallback variant,
/// for the reason `CONTRACT_VERSION` documents: an unfamiliar value must not poison the document
/// around it.
#[test]
fn an_unfamiliar_format_survives_a_round_trip_intact() {
    let contract = contract();
    let target = Target::document("config.hcl", Format::from_annotation("hcl"));
    let metadata = contract
        .kube_metadata(&target, &[&image(DIGEST_A)])
        .expect("an unknown format is still a format");

    assert_eq!(
        metadata
            .annotations()
            .get(ANNOTATION_FORMAT)
            .map(String::as_str),
        Some("hcl")
    );
    contract
        .verify_kube_metadata(&target, metadata.labels(), metadata.annotations())
        .expect("round trip");
    assert_eq!(Format::from_annotation("toml"), Format::Toml);
}

/// The block is pasted into a template, so it has to be byte-stable across runs for the same
/// reason `Contract::to_json` does: a generated artefact that differs run to run cannot be
/// committed and diffed.
#[test]
fn the_pasteable_block_is_byte_stable_and_indents_where_it_is_told() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[&image(DIGEST_A), &image(DIGEST_B)])
        .expect("stamped");

    assert_eq!(metadata.to_yaml(2), metadata.to_yaml(2));

    for indent in [0, 2, 8] {
        let rendered = metadata.to_yaml(indent);
        assert!(rendered.ends_with('\n'), "at {indent}: {rendered}");
        assert!(!rendered.contains('\t'), "a tab is not indentation in YAML");
        assert_indentation(&rendered, indent);
    }
}

/// Assert the shape of a rendered block: two mapping keys at `indent`, their entries at
/// `indent + 2`, and every entry a `key: "value"` pair.
fn assert_indentation(rendered: &str, indent: usize) {
    let outer = " ".repeat(indent);
    let inner = " ".repeat(indent + 2);

    let mut blocks = Vec::new();
    let mut entries = 0;
    for line in rendered.lines() {
        let depth = line.len() - line.trim_start().len();
        if depth == indent {
            assert!(
                line.ends_with(':'),
                "a block header takes no value: `{line}`"
            );
            assert!(line.starts_with(&outer));
            blocks.push(line.trim().trim_end_matches(':').to_owned());
        } else {
            assert_eq!(depth, indent + 2, "an entry sits two deeper: `{line}`");
            assert!(line.starts_with(&inner));
            let (key, value) = line
                .trim()
                .split_once(": ")
                .unwrap_or_else(|| panic!("not a mapping entry: `{line}`"));
            assert!(!key.is_empty() && !key.contains(' '), "`{key}`");
            assert!(
                value.starts_with('"') && value.ends_with('"'),
                "a label value is a string, not whatever YAML would infer: `{value}`"
            );
            entries += 1;
        }
    }

    assert_eq!(blocks, ["labels", "annotations"]);
    // Neither block is ever rendered as a bare key with no mapping under it.
    assert_eq!(entries, 4, "one label and three annotations");
}

/// A `Pairing` is only evidence when it was given both halves. Half of one is a check that would
/// otherwise pass by having nothing to disagree with.
#[test]
fn a_pairing_missing_a_half_refuses_rather_than_passes() {
    let contract = contract();
    let metadata = contract
        .kube_metadata(&document(), &[&image(DIGEST_A)])
        .expect("stamped");

    Pairing::new(&contract, DEFAULT_PATH)
        .object(&document(), metadata.labels(), metadata.annotations())
        .check()
        .expect_err("no image was given");

    Pairing::new(&contract, DEFAULT_PATH)
        .image(&image(DIGEST_A), &image_labels(&contract))
        .check()
        .expect_err("no object was given");
}

/// A stamp naming no image is not "unstamped" — it is a claim that this document is read by
/// nothing, which no pairing can ever satisfy and which reads as an accident.
#[test]
fn a_stamp_naming_no_image_is_refused_rather_than_written_empty() {
    let contract = contract();
    let error = contract
        .kube_metadata(&document(), &[])
        .expect_err("refused");
    assert!(error.to_string().contains(ANNOTATION_IMAGES), "{error}");

    let metadata: Metadata = contract
        .kube_metadata(&document(), &[&image(DIGEST_A)])
        .expect("stamped");
    let mut annotations = metadata.annotations().clone();
    annotations.insert(ANNOTATION_IMAGES.to_owned(), String::new());
    let error = contract
        .verify_kube_metadata(&document(), metadata.labels(), &annotations)
        .expect_err("refused");
    assert!(error.to_string().contains("empty"), "{error}");
}

/// A `ConfigMap`'s `data` is flat and the API server refuses `.` and `..` outright, so a key that
/// cannot be applied is caught where it is written rather than at `kubectl apply`.
#[test]
fn a_document_key_the_api_server_would_refuse_never_reaches_a_manifest() {
    let contract = contract();
    for key in [".", "..", "conf.d/config.toml", "config toml", ""] {
        contract
            .kube_metadata(&Target::document(key, Format::Toml), &[&image(DIGEST_A)])
            .expect_err("the API server would refuse this key");
    }
}
