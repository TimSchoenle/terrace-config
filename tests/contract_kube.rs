//! The cluster-side half of the contract: what a rendered object carries, and what pairs it with
//! the image.
//!
//! `contract.rs` checks what a build publishes about an *image*. This file checks the other end,
//! and the assertions divide into two kinds that fail very differently.
//!
//! The first is legality. Every key and value here goes into `metadata`, and the API server
//! applies rules the rest of this crate has never had to satisfy — a 63-byte label value that
//! must begin and end alphanumeric, a `ConfigMap` key that may not be `..`. A stamp that breaks
//! one of them fails at `kubectl apply`, after the chart rendered and after CI went green, with
//! a message naming neither this protocol nor the template line that produced it. So the
//! interesting fixture is not the tidy one: it is a contract whose prefix and app name are
//! *hostile* to the label rules, which is the case a real service walks into by naming itself
//! something ordinary.
//!
//! The second is the pairing. There a wrong answer is worse than a broken deployment: it is a
//! deployment that starts, mounting a document nothing checked against the binary reading it.
//! Every case below names both sides of the disagreement, because an error that says only
//! "mismatch" leaves whoever is holding the pager to go and find the other half themselves.

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
    App, CONTRACT_VERSION, Contract, DEFAULT_PATH, Describe, LABEL_PATH, LABEL_PREFIX,
    LABEL_VERSION, Schema,
};

/// The digests below are shaped like real ones so that a failure reads as a reference rather than
/// as a wall of hex. Nothing hashes to them and nothing needs to.
const WEB: &str =
    "ghcr.io/you/portfolio@sha256:48e2c1e7a4c0d4e6b2f8a1c3d5e7f9a1b3c5d7e9f1a3c5d7e9f1a3c5d7e9f1a3";
const WORKER: &str =
    "ghcr.io/you/worker@sha256:9f1c3e5a7b9d1f3a5c7e9b1d3f5a7c9e1b3d5f7a9c1e3b5d7f9a1c3e5b7d9f1c";
const CRON: &str =
    "ghcr.io/you/cron@sha256:1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b";
const STRANGER: &str =
    "ghcr.io/someone/else@sha256:0000111122223333444455556666777788889999aaaabbbbccccddddeeeeffff";

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

fn schema() -> Schema {
    Terrace::new("PORTFOLIO_")
        .reserve("PORTFOLIO_PROFILE")
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

/// The document object's target, as every well-formed case below stamps it.
fn document() -> Target {
    Target::document("config.toml", Format::Toml)
}

/// What `crane config` reports for an image built from `contract()`.
fn image_labels(contract: &Contract) -> BTreeMap<String, String> {
    contract
        .labels(DEFAULT_PATH)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

/// A pairing of a well-formed object and a well-formed image, ready to be broken one field at a
/// time.
fn stamp(contract: &Contract, images: &[&str]) -> Metadata {
    contract
        .kube_metadata(&document(), images)
        .expect("the stamp is one the API server accepts")
}

// ---------------------------------------------------------------------------------------------
// Legality
// ---------------------------------------------------------------------------------------------

/// Whether `value` is one the API server accepts as a label value.
///
/// Restated here rather than reached through the crate, and deliberately: the module under test
/// owns the only other copy, so a test calling it would agree with the implementation by
/// construction and prove nothing about the rule. This is the rule as the Kubernetes API
/// reference states it.
fn is_label_value(value: &str) -> bool {
    value.is_empty()
        || (value.len() <= 63
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
            && value.starts_with(|c: char| c.is_ascii_alphanumeric())
            && value.ends_with(|c: char| c.is_ascii_alphanumeric()))
}

/// Whether `key` is one the API server accepts for a label or an annotation.
fn is_key(key: &str) -> bool {
    let (prefix, name) = match key.split_once('/') {
        Some((prefix, name)) => (Some(prefix), name),
        None => (None, key),
    };
    let prefix_ok = prefix.is_none_or(|prefix| {
        !prefix.is_empty()
            && prefix.len() <= 253
            && prefix.split('.').all(|label| {
                !label.is_empty()
                    && label
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                    && label.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
                    && label.ends_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            })
    });
    prefix_ok && !name.is_empty() && !name.contains('/') && is_label_value(name)
}

#[test]
fn every_key_and_value_a_stamp_carries_is_one_kubernetes_accepts() {
    // The hostile fixture, and it is not contrived: `PORTFOLIO_` is what a service's prefix
    // ordinarily looks like and it ends in an underscore, so it is illegal as a label value. The
    // app name is worse on purpose — dots, a trailing dash, upper case, and 80 characters of it.
    // If any of that reached a label, this would be the test that says so instead of the API
    // server saying it after the release.
    let hostile = Terrace::new("PORTFOLIO_")
        .schema::<Config>()
        .with_defaults_from(&Config::default())
        .expect("serialises")
        .into_contract(
            App::new(format!("A.Very.{}-", "Long".repeat(20))).version("v2.5.0-rc.1+build.7"),
        )
        .build()
        .expect("nothing in a name is the contract's business");

    for target in [document(), Target::workload()] {
        let metadata = hostile
            .kube_metadata(&target, &[WEB, WORKER])
            .expect("a hostile name reaches no label");

        for (key, value) in metadata.labels() {
            assert!(is_key(key), "label key `{key}`");
            assert!(is_label_value(value), "label value `{value}` under `{key}`");
        }
        // An annotation value is unconstrained, so only the keys are checked — which is the whole
        // reason the prefix and the image list are annotations rather than labels.
        for key in metadata.annotations().keys() {
            assert!(is_key(key), "annotation key `{key}`");
        }
    }
}

#[test]
fn the_only_label_is_the_version_and_it_is_the_contracts_own() {
    let metadata = stamp(&contract(), &[WEB]);

    assert_eq!(
        metadata.labels(),
        &BTreeMap::from([(
            LABEL_CONTRACT_VERSION.to_owned(),
            CONTRACT_VERSION.to_string(),
        )])
    );
    // The two omissions this design is arranged around. Both are facts about an image, the image
    // already carries them, and a second spelling on an object no build-time check can see is a
    // second spelling that would be believed exactly when it has gone stale.
    let annotations = metadata.annotations();
    assert!(!annotations.contains_key("dev.terrace.config/prefix"));
    assert!(!annotations.contains_key("dev.terrace.config/contract-path"));
    assert!(!annotations.contains_key("dev.terrace.config/app"));
    // Everything this protocol writes shares one namespace, so a `kubectl get -o json | grep`
    // finds all of it and nothing of anybody else's.
    for key in metadata.labels().keys().chain(annotations.keys()) {
        assert!(key.starts_with(&format!("{NAMESPACE}/")), "{key}");
    }
}

#[test]
fn a_pod_template_carries_the_image_list_and_nothing_about_a_document() {
    let contract = contract();
    let workload = contract
        .kube_metadata(&Target::workload(), &[WEB, WORKER])
        .expect("stamps");

    // The label is there so a webhook can select the pod at all, and the image list is there so
    // it can decide what to check without walking ownership references back to the `ConfigMap`.
    assert!(workload.labels().contains_key(LABEL_CONTRACT_VERSION));
    assert_eq!(
        workload
            .annotations()
            .get(ANNOTATION_IMAGES)
            .map(String::as_str),
        Some(format!("{WEB},{WORKER}").as_str())
    );
    // A document key on a pod would be a claim about a file the pod does not hold.
    assert!(!workload.annotations().contains_key(ANNOTATION_DOCUMENT_KEY));
    assert!(!workload.annotations().contains_key(ANNOTATION_FORMAT));

    // And it is refused rather than ignored on the way back in: a pod carrying one is a chart
    // that pasted the `ConfigMap`'s block into the pod template, which means the image list
    // beside it is the document's rather than this workload's.
    let mut confused = workload.annotations().clone();
    confused.insert(ANNOTATION_DOCUMENT_KEY.to_owned(), "config.toml".to_owned());
    let error = contract
        .verify_kube_metadata(&Target::workload(), workload.labels(), &confused)
        .expect_err("a pod is not a document");
    assert!(
        error.to_string().contains(ANNOTATION_DOCUMENT_KEY),
        "{error}"
    );
    assert!(error.to_string().contains("pod"), "{error}");
}

#[test]
fn an_image_that_is_not_digest_pinned_is_refused() {
    let contract = contract();

    // A tag can be moved after the object was stamped, by somebody who never touched the chart,
    // and the pairing would still pass — against a binary reading different keys. This is the one
    // rule in the module that cannot be relaxed without making the whole check decorative.
    let error = contract
        .kube_metadata(&document(), &["ghcr.io/you/portfolio:v2.5.0"])
        .expect_err("a tag pins nothing");
    assert!(
        error.to_string().contains("ghcr.io/you/portfolio:v2.5.0"),
        "{error}"
    );
    assert!(error.to_string().contains("sha256"), "{error}");

    // One pinned member does not launder an unpinned one: the unpinned image is a hole for
    // whoever runs *it*, whichever member the caller happens to care about.
    assert!(
        contract
            .kube_metadata(&document(), &[WEB, "ghcr.io/you/worker:latest"])
            .is_err()
    );
    // A digest that is the right shape and the wrong alphabet. Upper case is a different string
    // to a registry, so it would never match rather than matching loosely.
    assert!(
        contract
            .kube_metadata(&document(), &["p@sha256:ABCD1234"])
            .is_err()
    );
    // And an object nothing claims to read is not a stamp, it is a decoration.
    assert!(contract.kube_metadata(&document(), &[]).is_err());
}

#[test]
fn a_document_key_that_names_a_directory_is_refused() {
    let contract = contract();

    for key in [".", "..", "", "etc/config.toml"] {
        assert!(
            contract
                .kube_metadata(&Target::document(key, Format::Toml), &[WEB])
                .is_err(),
            "`{key}` is not a key a `ConfigMap` can hold"
        );
    }
    // An unfamiliar format is a value a consumer reports as unrecognised, not one this crate
    // refuses to let a chart write — see `Format::Other`.
    assert!(contract.kube_metadata(&document(), &[WEB]).is_ok());
    assert!(
        contract
            .kube_metadata(&Target::document("config.hcl", Format::from("hcl")), &[WEB],)
            .is_ok()
    );
    // What is refused is a value nothing could dispatch a parser on, which is what a template
    // that interpolated nothing leaves behind.
    assert!(
        contract
            .kube_metadata(&Target::document("config.toml", Format::from("")), &[WEB])
            .is_err()
    );
}

// ---------------------------------------------------------------------------------------------
// The block a chart pastes
// ---------------------------------------------------------------------------------------------

#[test]
fn the_yaml_block_is_byte_stable_and_indents_where_it_is_told() {
    let metadata = stamp(&contract(), &[WEB]);

    // Byte-stable for the reason every other rendering in this crate is: the block is pasted into
    // a template and then diffed in review, and two identical stamps that render differently make
    // a diff out of nothing.
    assert_eq!(metadata.to_yaml(2), metadata.to_yaml(2));

    for indent in [0, 2, 8] {
        let rendered = metadata.to_yaml(indent);
        let outer = " ".repeat(indent);
        let inner = " ".repeat(indent + 2);

        assert!(rendered.ends_with('\n'), "at {indent}: {rendered}");
        assert!(!rendered.contains('\t'), "a tab is not indentation in YAML");

        let mut headings = Vec::new();
        for line in rendered.lines() {
            let depth = line.len() - line.trim_start().len();
            if depth == indent {
                // A mapping key at the block's own level, so it must be one of the two headings
                // and it must be exactly at `indent` — a heading one space out is a sibling of
                // whatever it was pasted under rather than a child.
                assert!(line.starts_with(&outer), "at {indent}: `{line}`");
                headings.push(line.trim().to_owned());
            } else {
                assert!(line.starts_with(&inner), "at {indent}: `{line}`");
                assert_eq!(depth, indent + 2, "at {indent}: `{line}`");
            }
        }
        assert_eq!(
            headings,
            vec!["labels:".to_owned(), "annotations:".to_owned()]
        );
    }

    // Values are quoted and keys are not. The version's value is `1`, and left bare YAML reads
    // that as an integer — which the API server refuses, because a label value is a string.
    let rendered = metadata.to_yaml(2);
    assert!(
        rendered.contains(&format!("  {LABEL_CONTRACT_VERSION}: \"1\"\n")),
        "{rendered}"
    );
}

// ---------------------------------------------------------------------------------------------
// The round trip
// ---------------------------------------------------------------------------------------------

#[test]
fn what_the_stamp_produced_is_what_the_check_accepts() {
    let contract = contract();

    for target in [document(), Target::workload()] {
        let metadata = contract
            .kube_metadata(&target, &[WEB, WORKER, CRON])
            .expect("stamps");
        contract
            .verify_kube_metadata(&target, metadata.labels(), metadata.annotations())
            .expect("a stamp this contract produced is one it accepts");
    }
}

#[test]
fn the_labels_and_annotations_a_chart_adds_of_its_own_are_ignored() {
    let contract = contract();
    let metadata = stamp(&contract, &[WEB]);

    // Every object in a real chart carries these, and none of it is this document's business —
    // the same tolerance `verify_labels` already extends to an image's `org.opencontainers.*`.
    let mut labels = metadata.labels().clone();
    labels.insert("app.kubernetes.io/name".to_owned(), "portfolio".to_owned());
    labels.insert(
        "app.kubernetes.io/instance".to_owned(),
        "staging".to_owned(),
    );
    labels.insert("helm.sh/chart".to_owned(), "portfolio-1.4.2".to_owned());

    let mut annotations = metadata.annotations().clone();
    annotations.insert(
        "checksum/config".to_owned(),
        "b1946ac92492d2347c6235b4d2611184".to_owned(),
    );

    contract
        .verify_kube_metadata(&document(), &labels, &annotations)
        .expect("a stranger's key is a stranger's business");
}

#[test]
fn a_stamp_the_chart_edited_by_hand_is_refused_and_the_message_names_both_sides() {
    let contract = contract();
    let metadata = stamp(&contract, &[WEB]);

    let mut annotations = metadata.annotations().clone();
    annotations.insert(
        ANNOTATION_DOCUMENT_KEY.to_owned(),
        "settings.toml".to_owned(),
    );
    let error = contract
        .verify_kube_metadata(&document(), metadata.labels(), &annotations)
        .expect_err("the key the annotation names is the file a validator reads");
    assert!(error.to_string().contains("settings.toml"), "{error}");
    assert!(error.to_string().contains("config.toml"), "{error}");

    let mut annotations = metadata.annotations().clone();
    annotations.insert(ANNOTATION_FORMAT.to_owned(), "yaml".to_owned());
    let error = contract
        .verify_kube_metadata(&document(), metadata.labels(), &annotations)
        .expect_err("the format decides which parser reads the document");
    assert!(error.to_string().contains("yaml"), "{error}");
    assert!(error.to_string().contains("toml"), "{error}");

    let mut labels = metadata.labels().clone();
    labels.remove(LABEL_CONTRACT_VERSION);
    let error = contract
        .verify_kube_metadata(&document(), &labels, metadata.annotations())
        .expect_err("nothing selecting on the label would ever see this object");
    assert!(
        error.to_string().contains(LABEL_CONTRACT_VERSION),
        "{error}"
    );
}

// ---------------------------------------------------------------------------------------------
// The pairing
// ---------------------------------------------------------------------------------------------

#[test]
fn a_document_and_the_image_that_reads_it_pair() {
    let contract = contract();
    let image_labels = image_labels(&contract);
    let metadata = stamp(&contract, &[WEB]);

    Pairing::new(
        &contract,
        WEB,
        &image_labels,
        metadata.labels(),
        metadata.annotations(),
    )
    .check()
    .expect("one image, one document, one configuration surface");
}

#[test]
fn every_image_in_a_union_pairs_and_a_fourth_does_not() {
    let contract = contract();
    let image_labels = image_labels(&contract);
    // The union case: one `ConfigMap` read by a web binary, a worker and a cron job, all built
    // from one source tree. Membership rather than equality is the whole reason `images` is an
    // annotation and not a label — a label value cannot hold a list at all.
    let metadata = stamp(&contract, &[WEB, WORKER, CRON]);

    for image in [WEB, WORKER, CRON] {
        Pairing::new(
            &contract,
            image,
            &image_labels,
            metadata.labels(),
            metadata.annotations(),
        )
        .check()
        .unwrap_or_else(|error| panic!("`{image}` reads this document: {error}"));
    }

    let error = Pairing::new(
        &contract,
        STRANGER,
        &image_labels,
        metadata.labels(),
        metadata.annotations(),
    )
    .check()
    .expect_err("a document rendered for three images says nothing about a fourth");
    // Both sides: the image that is running, and the list it is not in.
    assert!(error.to_string().contains(STRANGER), "{error}");
    assert!(error.to_string().contains(WEB), "{error}");
}

#[test]
fn a_version_skew_between_the_object_and_the_image_is_refused_and_both_sides_are_named() {
    let contract = contract();
    let metadata = stamp(&contract, &[WEB]);

    // The chart was rendered against this protocol and the image was built against the next one —
    // or the other way round. Either way the two are out of step, and checking them under rules
    // only one of them agreed to is the failure this pairing exists to prevent.
    let mut image_labels = image_labels(&contract);
    image_labels.insert(LABEL_VERSION.to_owned(), "2".to_owned());

    let error = Pairing::new(
        &contract,
        WEB,
        &image_labels,
        metadata.labels(),
        metadata.annotations(),
    )
    .check()
    .expect_err("a document and an image from two protocol versions do not pair");

    let message = error.to_string();
    assert!(message.contains('1'), "{message}");
    assert!(message.contains('2'), "{message}");
    // Named by their keys, so whoever is reading this at three in the morning knows which of the
    // two objects to go and look at.
    assert!(
        message.contains(LABEL_CONTRACT_VERSION) || message.contains(LABEL_VERSION),
        "{message}"
    );
}

#[test]
fn the_image_half_is_checked_by_the_one_implementation_that_already_existed() {
    let contract = contract();
    let metadata = stamp(&contract, &[WEB]);

    // `Contract::verify_labels` verbatim, not reimplemented: a build argument that failed to
    // interpolate, a base image that overrode a label, a `LABEL` line deleted on a branch nobody
    // diffed. Each is a failure the pairing inherits for free, and each would be a second thing
    // to keep in step if this module had its own copy of the rule.
    for (name, wrong) in [
        (LABEL_PREFIX, String::new()),
        (LABEL_PATH, "/etc/contract.json".to_owned()),
    ] {
        let mut image_labels = image_labels(&contract);
        image_labels.insert(name.to_owned(), wrong);

        let error = Pairing::new(
            &contract,
            WEB,
            &image_labels,
            metadata.labels(),
            metadata.annotations(),
        )
        .check()
        .expect_err("the image disagrees with the contract it claims to publish");
        assert!(error.to_string().contains(name), "{error}");
    }

    // The path is a build's choice, so a build that embedded the document elsewhere says so and
    // the same labels then pair.
    let elsewhere = contract
        .labels("/etc/contract.json")
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<BTreeMap<_, _>>();
    Pairing::new(
        &contract,
        WEB,
        &elsewhere,
        metadata.labels(),
        metadata.annotations(),
    )
    .embedded_at("/etc/contract.json")
    .check()
    .expect("a build that embedded the document elsewhere pairs by saying so");
}

#[test]
fn a_pairing_will_not_read_an_object_that_never_joined_the_protocol() {
    let contract = contract();
    let image_labels = image_labels(&contract);
    let metadata = stamp(&contract, &[WEB]);

    // A pod mounting somebody else's `ConfigMap`. Nothing about it is wrong as a Kubernetes
    // object; what is wrong is treating it as a document this contract describes.
    let error = Pairing::new(
        &contract,
        WEB,
        &image_labels,
        &BTreeMap::new(),
        metadata.annotations(),
    )
    .check()
    .expect_err("an unstamped object claims nothing");
    assert!(
        error.to_string().contains(LABEL_CONTRACT_VERSION),
        "{error}"
    );

    // And an object whose image list went missing, which is a stamp half-written rather than a
    // stamp absent — the case a validator must not read as "no images to check, so nothing to
    // check".
    let mut annotations = metadata.annotations().clone();
    annotations.remove(ANNOTATION_IMAGES);
    let error = Pairing::new(
        &contract,
        WEB,
        &image_labels,
        metadata.labels(),
        &annotations,
    )
    .check()
    .expect_err("an absent image list is not an empty one");
    assert!(error.to_string().contains(ANNOTATION_IMAGES), "{error}");
}

#[test]
fn a_list_a_template_spaced_out_still_pairs_and_a_mangled_reference_does_not() {
    let contract = contract();
    let image_labels = image_labels(&contract);
    let metadata = stamp(&contract, &[WEB]);

    // A hand-written or templated list separated with `, ` is common enough that refusing it
    // would fail deployments over whitespace rather than over a pairing.
    let mut annotations = metadata.annotations().clone();
    annotations.insert(ANNOTATION_IMAGES.to_owned(), format!("{WEB}, {WORKER}"));
    Pairing::new(
        &contract,
        WORKER,
        &image_labels,
        metadata.labels(),
        &annotations,
    )
    .check()
    .expect("a space after the comma is not a different image");

    // A reference that still carries whitespace after the trim is a template that interpolated
    // nothing, and it is refused like any other malformed one.
    let mut annotations = metadata.annotations().clone();
    annotations.insert(
        ANNOTATION_IMAGES.to_owned(),
        "ghcr.io/you/ portfolio".to_owned(),
    );
    assert!(
        Pairing::new(
            &contract,
            WEB,
            &image_labels,
            metadata.labels(),
            &annotations,
        )
        .check()
        .is_err()
    );
}
