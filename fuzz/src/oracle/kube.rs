//! Does everything the Kubernetes stamp emits survive `kubectl apply`?
//!
//! The other oracles run the loader and check what came back. This one checks a claim the crate
//! makes about a platform it cannot call: that every key and value
//! [`Contract::kube_metadata`](terrace_config::schema::Contract::kube_metadata) produces is legal
//! Kubernetes object metadata. Nothing in the test suite can ask an API server, so the only way to
//! hold the claim is to state the rules independently and run them over everything the builder
//! will accept.
//!
//! That independence is the point. The rules below are a **second implementation**, deliberately
//! not the crate's: an oracle that asked the code under test whether it was right would agree with
//! any answer it gave.
//!
//! # Oracle
//! 1. **Legality.** For any contract the builder accepts and any inputs `kube_metadata` accepts,
//!    every key it emits is a legal label/annotation key and every *label* value is a legal label
//!    value. Annotation values are unconstrained by the platform and are not held to the rule.
//! 2. **Round trip.** `verify_kube_metadata` accepts what `kube_metadata` produced — for both
//!    targets, so the workload's strict subset is exercised as well as the document's full stamp.
//! 3. **The pairing closes.** The metadata, paired with the image labels the same contract
//!    publishes and any one of the references it was stamped with, checks out. A stamp that could
//!    not be verified against the image it names would be a protocol with no consumer.
//! 4. **Refusal is not optional.** Where this oracle's own rules say an input is illegal — an
//!    unpinned reference, a `ConfigMap` key the API server refuses — `kube_metadata` must return
//!    an error rather than a stamp. The direction that catches a validator quietly getting
//!    laxer.
//! 5. **Determinism.** Two renders of one stamp are byte-identical, which is what lets a generated
//!    block be committed and diffed.
//!
//! # The input
//!
//! ```text
//! p:<prefix>          the loader's environment prefix, default `TEST_`
//! a:<name>            the app name
//! i:<reference>       an image that reads the document; repeatable
//! d:<key>             the `ConfigMap` data key the document is
//! f:<format>          how the document is written
//! w:                  stamp the workload's pod template instead of the document
//! ```
//!
//! Nothing about the *keys* of a configuration reaches a Kubernetes object, so this grammar
//! describes none. What reaches one is the envelope version and the four caller-supplied strings,
//! and those are what it varies.

use std::collections::BTreeMap;

use terrace_config::Terrace;
use terrace_config::schema::kube::{
    ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT, ANNOTATION_IMAGES, Format, LABEL_CONTRACT_VERSION,
    Metadata, Pairing, Target,
};
use terrace_config::schema::{App, CONTRACT_VERSION, Contract, DEFAULT_PATH, Describe, Leaf, Sink};

use crate::support::{MAX_DIRECTIVES, MAX_NAME_LEN, PREFIX};

/// The most images one stamp will name. Past a handful the list stops testing anything new.
const MAX_IMAGES: usize = 6;

/// A stand-in for a consumer's config type.
///
/// Fixed rather than fuzzed, and that is the finding rather than a shortcut: no configuration key
/// reaches a Kubernetes object. If one ever does, this oracle keeps passing and the test above it
/// in `tests/contract_kube.rs` — which asserts the hostile prefix is absent from the stamp — is
/// the one that fails.
struct Fixed;

impl Describe for Fixed {
    fn describe(sink: &mut Sink) {
        sink.leaf(Leaf {
            name: "dist_dir",
            docs: "Bundle directory the readiness probe checks.",
            ty: Some("String"),
            values: None,
            aliases: &[],
            note: None,
            required: false,
            secret: false,
        });
    }
}

/// What the input asked for.
#[derive(Debug, Default)]
struct Spec {
    prefix: Option<String>,
    app: Option<String>,
    images: Vec<String>,
    document_key: Option<String>,
    format: Option<String>,
    workload: bool,
}

/// Parse the line grammar, skipping anything that does not fit it.
fn parse(data: &str) -> Spec {
    let mut spec = Spec::default();
    for line in data.lines().take(MAX_DIRECTIVES) {
        let Some((kind, rest)) = line.split_once(':') else {
            continue;
        };
        // A `sha256:` digest carries the separator this grammar splits on, so an `i:` directive
        // takes everything after the first colon and is never split again.
        match kind {
            "p" if rest.len() <= MAX_NAME_LEN => spec.prefix = Some(rest.to_owned()),
            "a" if rest.len() <= MAX_NAME_LEN => spec.app = Some(rest.to_owned()),
            "i" if rest.len() <= MAX_NAME_LEN && spec.images.len() < MAX_IMAGES => {
                spec.images.push(rest.to_owned());
            }
            "d" if rest.len() <= MAX_NAME_LEN => spec.document_key = Some(rest.to_owned()),
            "f" if rest.len() <= MAX_NAME_LEN => spec.format = Some(rest.to_owned()),
            "w" => spec.workload = true,
            _ => {}
        }
    }
    spec
}

/// Run the oracle. Panics when the stamp breaks one of the rules above.
///
/// # Panics
/// That is the contract: a panic is the finding.
pub fn check(data: &str) {
    let spec = parse(data);

    let prefix = spec.prefix.as_deref().unwrap_or(PREFIX);
    let app = spec.app.as_deref().unwrap_or("portfolio");
    // A contract the builder refuses says nothing about the stamp — an empty prefix is refused by
    // `validate_dialect`, and that refusal is `contract.rs`'s property, not this one's.
    let Ok(contract) = Terrace::new(prefix)
        .schema::<Fixed>()
        .into_contract(App::new(app))
        .build()
    else {
        return;
    };

    let target = if spec.workload {
        Target::Workload
    } else {
        Target::document(
            spec.document_key.as_deref().unwrap_or("config.toml"),
            spec.format
                .as_deref()
                .map_or(Format::Toml, Format::from_annotation),
        )
    };

    let images: Vec<&str> = if spec.images.is_empty() {
        vec![PINNED]
    } else {
        spec.images.iter().map(String::as_str).collect()
    };

    let outcome = contract.kube_metadata(&target, &images);

    // (4) The refusal direction. Everything this oracle can see to be illegal must have been
    // refused, so a validator that quietly stopped checking is a failure here rather than a stamp
    // nobody can apply.
    let unpinned = images.iter().find(|image| !pinned(image));
    if let Some(image) = unpinned {
        assert!(
            outcome.is_err(),
            "`{image}` is not digest-pinned and was stamped anyway; a pairing keyed on anything a \
             tag can move is not a pairing"
        );
    }
    if let Target::Document { key, .. } = &target
        && !legal_configmap_key(key)
    {
        assert!(
            outcome.is_err(),
            "`{key}` is not a key a `ConfigMap` can hold, and it was stamped anyway; the API \
             server refuses it at apply time, where the message names neither the chart nor this \
             contract"
        );
    }

    let Ok(metadata) = outcome else {
        return;
    };

    legality(&metadata);
    round_trip(&contract, &target, &metadata);
    the_pairing_closes(&contract, &target, &metadata, &images);
    determinism(&metadata);
}

/// A reference every default path uses, so an input that names no image still exercises the rest.
const PINNED: &str =
    "ghcr.io/you/portfolio@sha256:48e259cb4c9d0e3f1a2b5c6d7e8f9012345678901234567890abcdefabcdef01";

/// (1) Everything emitted is metadata Kubernetes accepts.
fn legality(metadata: &Metadata) {
    for (key, value) in metadata.labels() {
        assert!(legal_key(key), "`{key}` is not a legal Kubernetes key");
        assert!(
            legal_label_value(value),
            "`{key}` was stamped with `{value}`, which is not a legal label value — \
             `kubectl apply` refuses the whole object"
        );
    }
    for key in metadata.annotations().keys() {
        assert!(
            legal_key(key),
            "`{key}` is not a legal Kubernetes annotation key"
        );
    }

    // The label surface is exactly one entry. A second one appearing here would be a fact this
    // module decided to make selectable without anybody deciding it could be spelled legally.
    assert_eq!(
        metadata.labels().len(),
        1,
        "the label surface is one entry: {:?}",
        metadata.labels()
    );
    // The version is read through the crate rather than copied, so a bump cannot leave this
    // oracle agreeing with a stamp that stopped saying what it says.
    assert_eq!(
        metadata
            .labels()
            .get(LABEL_CONTRACT_VERSION)
            .map(String::as_str),
        Some(CONTRACT_VERSION.to_string().as_str()),
        "the one label carries the envelope version and nothing else"
    );
    assert!(metadata.annotations().contains_key(ANNOTATION_IMAGES));
}

/// (2) The verifier accepts what the generator produced, and a workload carries the strict subset.
fn round_trip(contract: &Contract, target: &Target, metadata: &Metadata) {
    contract
        .verify_kube_metadata(target, metadata.labels(), metadata.annotations())
        .expect("a stamp this crate produced must be a stamp this crate accepts");

    match target {
        Target::Document { .. } => {
            assert!(
                metadata.annotations().contains_key(ANNOTATION_DOCUMENT_KEY)
                    && metadata.annotations().contains_key(ANNOTATION_FORMAT),
                "a document says which file it is and how it is written"
            );
        }
        Target::Workload => {
            assert!(
                !metadata.annotations().contains_key(ANNOTATION_DOCUMENT_KEY)
                    && !metadata.annotations().contains_key(ANNOTATION_FORMAT),
                "a pod is not a document, and a copy of a document's stamp goes stale unnoticed"
            );
        }
        // The enum is `#[non_exhaustive]`; a variant this oracle has not been taught about is not
        // something to guess at.
        _ => {}
    }
}

/// (3) The stamp pairs with the image labels the same contract publishes, for every image it names.
fn the_pairing_closes(contract: &Contract, target: &Target, metadata: &Metadata, images: &[&str]) {
    let labels: BTreeMap<String, String> = contract
        .labels(DEFAULT_PATH)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();

    for image in images {
        Pairing::new(contract, DEFAULT_PATH)
            .image(image, &labels)
            .object(target, metadata.labels(), metadata.annotations())
            .check()
            .unwrap_or_else(|error| {
                panic!("`{image}` is named by this stamp and does not pair with it: {error}")
            });
    }

    // And an image nobody listed does not pair. Built by mangling a digest rather than by picking
    // a constant, so it cannot collide with something the input happened to name.
    let stranger = format!("ghcr.io/nobody/listed-this@sha256:{}", "f".repeat(64));
    if !images.contains(&stranger.as_str()) {
        Pairing::new(contract, DEFAULT_PATH)
            .image(&stranger, &labels)
            .object(target, metadata.labels(), metadata.annotations())
            .check()
            .expect_err("an image this document was never rendered for must not pair with it");
    }
}

/// (5) Two renders of one stamp are the same bytes.
fn determinism(metadata: &Metadata) {
    for indent in [0, 2, 8] {
        assert_eq!(
            metadata.to_yaml(indent),
            metadata.to_yaml(indent),
            "a block that differs between two renders cannot be committed and diffed"
        );
    }
    // Indentation is the only difference between two widths, so a wider render is the narrower
    // one with every line shifted.
    let narrow = metadata.to_yaml(0);
    let wide = metadata.to_yaml(4);
    let mut shifted = String::new();
    for line in narrow.lines() {
        shifted.push_str("    ");
        shifted.push_str(line);
        shifted.push('\n');
    }
    assert_eq!(wide, shifted, "`to_yaml` shifts, it does not reflow");
}

// --- the platform's rules, restated ----------------------------------------------------------
//
// A second implementation on purpose. See the module documentation.

/// Whether `value` is a legal Kubernetes label value.
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
    if let Some(prefix) = prefix
        && !legal_subdomain(prefix)
    {
        return false;
    }
    !name.is_empty() && legal_label_value(name)
}

/// Whether `value` is a DNS subdomain of at most 253 characters.
fn legal_subdomain(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            let bytes = label.as_bytes();
            !label.is_empty()
                && bytes
                    .iter()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
                && bytes[0] != b'-'
                && bytes[bytes.len() - 1] != b'-'
        })
}

/// Whether `value` is a key a `ConfigMap` can hold in `data`.
fn legal_configmap_key(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= 253
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Whether `reference` names an image a moved tag cannot change.
fn pinned(reference: &str) -> bool {
    if reference.contains(',') || reference.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((name, digest)) = reference.rsplit_once('@') else {
        return false;
    };
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return false;
    };
    !name.is_empty()
        && hex.len() == 64
        && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
