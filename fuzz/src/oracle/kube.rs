//! Is every stamp `kube_metadata` produces one the API server would actually accept?
//!
//! The other oracles fuzz the loader, and `schema` fuzzes the claim made about it. This one
//! fuzzes a claim made about a *third* system: Kubernetes applies its own rules to
//! `metadata.labels` and `metadata.annotations`, and nothing in this crate's test suite runs an
//! API server. A stamp that breaks one of those rules compiles, renders, passes CI, and fails at
//! `kubectl apply` — with a message that names neither this protocol nor the template line that
//! produced it.
//!
//! So the property is not "the module is internally consistent". It is that for **any** contract
//! the builder accepts and any inputs the stamp accepts:
//!
//! 1. every key it emits satisfies the key rule — a DNS-subdomain prefix of at most 253 bytes, a
//!    `/`, and a name of at most 63 bytes under the value rule;
//! 2. every **label** value satisfies the value rule, which is the tight one;
//! 3. a [`Target::Workload`] stamp carries nothing that describes a document;
//! 4. `verify_kube_metadata` accepts what `kube_metadata` produced — the round trip, which is
//!    what makes the two halves one protocol rather than two;
//! 5. the pasted block is byte-stable and indented where it was told;
//! 6. every image the stamp lists pairs with the contract that produced it.
//!
//! The rules in 1 and 2 are **restated here** rather than reached through the crate. An oracle
//! that called the module's own predicate would agree with it by construction and would find
//! nothing; this is the rule as the Kubernetes API reference states it, which is the thing the
//! module is claiming to have implemented.
//!
//! A refusal is always allowed. `kube_metadata` returning an error is the module declining to
//! emit something, and there is no input a fuzzer can produce that makes a refusal wrong — the
//! bug this hunts for is the opposite, an illegal stamp emitted happily.
//!
//! # The input
//!
//! ```text
//! p:<prefix>          the loader's environment prefix, default `TEST_`
//! a:<name>            the app's name
//! k:<a>/<b>           declare a leaf at `a.b`
//! K:<key>             the document key
//! F:<format>          the document format
//! i:<reference>       an image that reads the document, repeatable
//! w                   stamp the pod template instead of the document object
//! ```

use std::collections::BTreeMap;

use terrace_config::Terrace;
use terrace_config::schema::kube::{
    ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT, Format, Metadata, Pairing, Target,
};
use terrace_config::schema::{App, Contract, DEFAULT_PATH, Describe, Leaf, Sink};

use crate::support::{MAX_DIRECTIVES, MAX_NAME_LEN, PREFIX};

/// The most leaves one iteration will describe. The schema half is not what this oracle is
/// about; it is here so that the contract under test is a real one rather than one shape.
const MAX_LEAVES: usize = 8;

/// The most images one iteration will list. An unbounded list is an unbounded string to check
/// and finds nothing a short one does not.
const MAX_IMAGES: usize = 8;

/// What the input asked for.
#[derive(Debug, Default)]
struct Spec {
    prefix: Option<String>,
    app: Option<String>,
    leaves: Vec<Vec<String>>,
    key: Option<String>,
    format: Option<String>,
    images: Vec<String>,
    workload: bool,
}

impl Spec {
    /// The object this input stamps.
    fn target(&self) -> Target {
        if self.workload {
            Target::workload()
        } else {
            Target::document(
                self.key.clone().unwrap_or_else(|| "config.toml".to_owned()),
                self.format.as_deref().map_or(Format::Toml, Format::from),
            )
        }
    }
}

thread_local! {
    /// The leaves the current iteration is describing.
    ///
    /// `Describe::describe` takes no value, so a fuzzer-driven implementation has nowhere else to
    /// read its input from. Thread local rather than global because the replay suite runs the
    /// oracles in parallel.
    static LEAVES: std::cell::RefCell<Vec<Vec<String>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// A stand-in for a consumer's config type, whose keys come from the input.
struct Fuzzed;

impl Describe for Fuzzed {
    fn describe(sink: &mut Sink) {
        LEAVES.with_borrow(|leaves| {
            for segments in leaves {
                describe_one(sink, segments);
            }
        });
    }
}

/// Walk `segments`, opening a subtree per segment and reporting the leaf at the end.
fn describe_one(sink: &mut Sink, segments: &[String]) {
    match segments {
        [] => {}
        [name] => sink.leaf(Leaf {
            name,
            docs: "",
            ty: Some("String"),
            values: None,
            aliases: &[],
            note: None,
            required: false,
            secret: false,
        }),
        [head, tail @ ..] => sink.nested(head, |sink| describe_one(sink, tail)),
    }
}

/// Parse the line grammar, skipping anything that does not fit it.
///
/// A malformed line is skipped rather than rejected so that a mutation corrupting one line still
/// exercises the rest instead of collapsing the iteration into a no-op.
fn parse(data: &str) -> Spec {
    let mut spec = Spec::default();
    let mut seen: Vec<String> = Vec::new();

    for line in data.lines().take(MAX_DIRECTIVES) {
        if line == "w" {
            spec.workload = true;
            continue;
        }
        let Some((kind, rest)) = line.split_once(':') else {
            continue;
        };
        if rest.len() > MAX_NAME_LEN {
            continue;
        }
        match kind {
            "p" => spec.prefix = Some(rest.to_owned()),
            "a" => spec.app = Some(rest.to_owned()),
            "K" => spec.key = Some(rest.to_owned()),
            "F" => spec.format = Some(rest.to_owned()),
            "i" if spec.images.len() < MAX_IMAGES => spec.images.push(rest.to_owned()),
            "k" if spec.leaves.len() < MAX_LEAVES => {
                let segments: Vec<String> = rest
                    .split('/')
                    .filter(|segment| !segment.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
                // A duplicate path is what `Sink::leaf` panics on by design, so feeding it would
                // be fuzzing the assertion rather than the code under it.
                let joined = segments.join(".");
                if !segments.is_empty() && !seen.contains(&joined) {
                    seen.push(joined);
                    spec.leaves.push(segments);
                }
            }
            _ => {}
        }
    }
    spec
}

/// The contract this input describes, or the builder's refusal.
fn build(spec: &Spec) -> Result<Contract, terrace_config::Error> {
    let prefix = spec.prefix.clone().unwrap_or_else(|| PREFIX.to_owned());
    let app = App::new(spec.app.clone().unwrap_or_else(|| "fuzzed".to_owned()));

    let schema = LEAVES.with(|cell| {
        cell.replace(spec.leaves.clone());
        let schema = Terrace::new(prefix).schema::<Fuzzed>();
        cell.replace(Vec::new());
        schema
    });

    schema.into_contract(app).build()
}

/// Run every property over one input.
pub fn check(data: &str) {
    let spec = parse(data);
    let Ok(contract) = build(&spec) else {
        return;
    };

    let target = spec.target();
    let images: Vec<&str> = spec.images.iter().map(String::as_str).collect();
    // A refusal is always allowed: the bug hunted here is a stamp emitted, not one declined.
    let Ok(metadata) = contract.kube_metadata(&target, &images) else {
        return;
    };

    every_key_and_value_is_one_kubernetes_accepts(&metadata);
    a_pod_carries_nothing_that_describes_a_document(&spec, &metadata);
    verify_accepts_what_the_stamp_produced(&contract, &target, &metadata);
    the_pasted_block_is_stable_and_indented(&metadata);
    every_listed_image_pairs_with_the_contract(&spec, &contract, &metadata);
}

/// Property 1 and 2: the API server would take all of it.
fn every_key_and_value_is_one_kubernetes_accepts(metadata: &Metadata) {
    for (key, value) in metadata.labels() {
        assert!(
            is_key(key),
            "label key `{key}` is not one Kubernetes accepts"
        );
        assert!(
            is_label_value(value),
            "label value `{value}` under `{key}` is not one Kubernetes accepts"
        );
    }
    // Only the keys: an annotation value is unconstrained, which is the whole reason the image
    // list and the document key are annotations rather than labels.
    for key in metadata.annotations().keys() {
        assert!(
            is_key(key),
            "annotation key `{key}` is not one Kubernetes accepts"
        );
    }
}

/// Property 3: a pod is not a document, so no annotation on one may claim it is.
fn a_pod_carries_nothing_that_describes_a_document(spec: &Spec, metadata: &Metadata) {
    if !spec.workload {
        return;
    }
    for name in [ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT] {
        assert!(
            !metadata.annotations().contains_key(name),
            "a pod template carries `{name}`, which describes a document"
        );
    }
}

/// Property 4: the two halves are one protocol.
///
/// The failure this catches is a stamp that emits one spelling and a check that expects another —
/// two functions in one module drifting apart, which is the defect the whole crate is arranged
/// to make impossible elsewhere.
fn verify_accepts_what_the_stamp_produced(
    contract: &Contract,
    target: &Target,
    metadata: &Metadata,
) {
    if let Err(error) =
        contract.verify_kube_metadata(target, metadata.labels(), metadata.annotations())
    {
        panic!("a stamp this contract produced is one it refuses: {error}");
    }
}

/// Property 5: the block a chart pastes is the same block twice, and lands where it was told.
fn the_pasted_block_is_stable_and_indented(metadata: &Metadata) {
    for indent in [0, 2, 8] {
        let rendered = metadata.to_yaml(indent);
        assert_eq!(
            rendered,
            metadata.to_yaml(indent),
            "two calls, two blocks, at indent {indent}"
        );
        assert!(rendered.ends_with('\n'), "at indent {indent}: {rendered}");
        assert!(
            !rendered.contains('\t'),
            "a tab is not indentation in YAML: {rendered}"
        );

        for line in rendered.lines() {
            let depth = line.len() - line.trim_start().len();
            assert!(
                depth == indent || depth == indent + 2,
                "at indent {indent}, `{line}` sits at {depth}"
            );
        }
    }
}

/// Property 6: every image the document says reads it can be paired with it.
///
/// Only for a document stamp, because a pairing is about a document — a pod template carries
/// neither annotation the fifth check reads.
fn every_listed_image_pairs_with_the_contract(
    spec: &Spec,
    contract: &Contract,
    metadata: &Metadata,
) {
    if spec.workload {
        return;
    }
    let image_labels: BTreeMap<String, String> = contract
        .labels(DEFAULT_PATH)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();

    for image in &spec.images {
        if let Err(error) = Pairing::new(
            contract,
            image,
            &image_labels,
            metadata.labels(),
            metadata.annotations(),
        )
        .check()
        {
            panic!("`{image}` is in this document's own image list and does not pair: {error}");
        }
    }
}

/// Whether `value` is one Kubernetes accepts as a **label value**.
///
/// At most 63 bytes and, unless empty, `(([A-Za-z0-9][-A-Za-z0-9_.]*)?[A-Za-z0-9])?`.
fn is_label_value(value: &str) -> bool {
    value.is_empty()
        || (value.len() <= 63
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
            && value.starts_with(|c: char| c.is_ascii_alphanumeric())
            && value.ends_with(|c: char| c.is_ascii_alphanumeric()))
}

/// Whether `key` is one Kubernetes accepts for a label or an annotation.
///
/// An optional DNS-subdomain prefix of at most 253 bytes, a `/`, then a non-empty name of at
/// most 63 bytes under the value rule.
fn is_key(key: &str) -> bool {
    let (prefix, name) = match key.split_once('/') {
        Some((prefix, name)) => (Some(prefix), name),
        None => (None, key),
    };
    let prefix_ok = prefix.is_none_or(|prefix| {
        let alphanumeric = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
        !prefix.is_empty()
            && prefix.len() <= 253
            && prefix.split('.').all(|label| {
                !label.is_empty()
                    && label.chars().all(|c| alphanumeric(c) || c == '-')
                    && label.starts_with(alphanumeric)
                    && label.ends_with(alphanumeric)
            })
    });
    prefix_ok && !name.is_empty() && !name.contains('/') && is_label_value(name)
}
