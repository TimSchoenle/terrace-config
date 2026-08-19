//! Can a stamp this crate emits be rejected by a Kubernetes API server?
//!
//! The `schema` oracle asks whether a spelling the crate advertises actually reaches its key. This
//! one asks a narrower question with a worse failure mode: **is every key and value
//! `kube_metadata` produces one that Kubernetes will accept?**
//!
//! It is worse because of where it fails. A wrong environment spelling fails when somebody sets it
//! and nothing happens. An illegal label value fails at `kubectl apply`, on an object a chart
//! rendered, in a pipeline — and it fails for the whole object, not for the label, so a chart that
//! deployed yesterday stops deploying because a service changed its prefix. The rules are also
//! unusually easy to violate by accident: the prefix this crate already publishes as an image
//! label (`PORTFOLIO_`) is illegal as a Kubernetes label value, and so is the contract path, and
//! so is the artifact type. Three of the four strings nearby are traps.
//!
//! Two properties, over every contract the builder accepts:
//!
//! 1. **Legality.** Every key `kube_metadata` emits satisfies the Kubernetes key rule, and every
//!    *label* value satisfies the label-value rule. Annotation values are deliberately not
//!    checked against it — being unconstrained is the whole reason the image list lives there.
//! 2. **Round trip.** `verify_kube_metadata` accepts exactly what `kube_metadata` produced. A
//!    stamp the crate's own checker rejects is a gate that fails on a correct deployment, which
//!    is the way a gate gets turned off.
//!
//! The rules in [`is_legal_key`] and [`is_legal_label_value`] are **restated here rather than
//! called**. An oracle that asked the code under test what is legal would agree with it by
//! construction, and this is the one place whose whole job is to disagree.
//!
//! # The input
//!
//! ```text
//! p:<prefix>          the loader prefix, default `TEST_`
//! a:<name>            the app name
//! k:<a>/<b>=docs      declare a leaf at `a.b`
//! d:<key>             the document key, default `config.toml`
//! f:<format>          the document format, default `toml`
//! i:<reference>       an image that reads the document; repeatable
//! w:                  stamp the workload pod template instead of the document
//! ```
//!
//! The prefix and the app name are in the grammar because they are the two strings a future
//! change is most likely to reach for when somebody wants "a bit more context on the object" —
//! and both are arbitrary text that no label rule constrains.

use std::collections::BTreeSet;

use terrace_config::Terrace;
use terrace_config::schema::kube::{Format, Metadata, Target};
use terrace_config::schema::{App, Contract, Describe, Leaf, Sink};

use crate::support::{MAX_DIRECTIVES, MAX_NAME_LEN, PREFIX};

/// The most leaves one iteration will describe.
const MAX_LEAVES: usize = 12;

/// The deepest a declared path will nest.
const MAX_DEPTH: usize = 4;

/// The most images one document will declare, well past the eight-binary union this exists for.
const MAX_IMAGES: usize = 16;

/// One leaf, as the input asked for it.
#[derive(Debug, Clone, Default)]
struct LeafSpec {
    segments: Vec<String>,
    docs: String,
}

/// What the input asked for, in full.
#[derive(Debug, Default)]
struct Spec {
    prefix: Option<String>,
    app: Option<String>,
    leaves: Vec<LeafSpec>,
    document_key: Option<String>,
    format: Option<String>,
    images: Vec<String>,
    workload: bool,
}

thread_local! {
    /// The leaves the current iteration is describing.
    ///
    /// [`Describe::describe`] takes no value, so a fuzzer-driven implementation has nowhere else
    /// to read its input from. Thread local rather than global because the replay suite runs
    /// oracles in parallel.
    static LEAVES: std::cell::RefCell<Vec<LeafSpec>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A stand-in for a consumer's config type, whose keys come from the input.
struct Fuzzed;

impl Describe for Fuzzed {
    fn describe(sink: &mut Sink) {
        LEAVES.with_borrow(|leaves| {
            for leaf in leaves {
                describe_one(sink, &leaf.segments, leaf);
            }
        });
    }
}

/// Walk `segments`, opening a subtree per segment and reporting the leaf at the end.
fn describe_one(sink: &mut Sink, segments: &[String], leaf: &LeafSpec) {
    match segments {
        [] => unreachable!("a spec always has at least one segment"),
        [name] => sink.leaf(Leaf {
            name,
            docs: &leaf.docs,
            ty: Some("String"),
            values: None,
            aliases: &[],
            note: None,
            required: false,
            secret: false,
        }),
        [head, tail @ ..] => sink.nested(head, |sink| describe_one(sink, tail, leaf)),
    }
}

/// Parse the line grammar, skipping anything that does not fit it.
fn parse(data: &str) -> Spec {
    let mut spec = Spec::default();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for line in data.lines().take(MAX_DIRECTIVES) {
        let Some((kind, rest)) = line.split_once(':') else {
            continue;
        };
        match kind {
            "p" if rest.len() <= MAX_NAME_LEN => spec.prefix = Some(rest.to_owned()),
            "a" if rest.len() <= MAX_NAME_LEN => spec.app = Some(rest.to_owned()),
            "d" if rest.len() <= MAX_NAME_LEN => spec.document_key = Some(rest.to_owned()),
            "f" if rest.len() <= MAX_NAME_LEN => spec.format = Some(rest.to_owned()),
            "w" => spec.workload = true,
            "i" if !rest.is_empty() && rest.len() <= MAX_NAME_LEN => {
                if spec.images.len() < MAX_IMAGES {
                    spec.images.push(rest.to_owned());
                }
            }
            "k" => {
                let Some((path, docs)) = rest.split_once('=') else {
                    continue;
                };
                let Some(segments) = segments(path) else {
                    continue;
                };
                // Keyed on the joined path, which is what `Sink` keys on: a duplicate
                // `Sink::leaf` panics by design, and feeding it would be fuzzing the assertion
                // rather than the code under it.
                let joined = segments.join(".");
                if spec.leaves.len() >= MAX_LEAVES || seen.contains(&joined) {
                    continue;
                }
                seen.insert(joined);
                spec.leaves.push(LeafSpec {
                    segments,
                    docs: docs.to_owned(),
                });
            }
            _ => {}
        }
    }
    spec
}

/// The path segments a `k:` directive names, or [`None`] if it names nothing usable.
fn segments(path: &str) -> Option<Vec<String>> {
    let segments: Vec<String> = path
        .split('/')
        .take(MAX_DEPTH)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    (!segments.is_empty() && segments.iter().all(|s| s.len() <= MAX_NAME_LEN)).then_some(segments)
}

/// Run every property over one input.
pub fn check(data: &str) {
    let spec = parse(data);
    if spec.leaves.is_empty() {
        return;
    }

    let prefix = spec.prefix.clone().unwrap_or_else(|| PREFIX.to_owned());
    let leaves = spec.leaves.clone();
    let schema = LEAVES.with(|cell| {
        cell.replace(leaves);
        let schema = Terrace::new(prefix).schema::<Fuzzed>();
        cell.replace(Vec::new());
        schema
    });

    // A refused contract is a legitimate outcome — an empty prefix is refused by design — and
    // says nothing about the stamp. The claim is about contracts the builder *accepts*.
    let app = App::new(spec.app.clone().unwrap_or_else(|| "fuzzed".to_owned()));
    let Ok(contract) = schema.into_contract(app).build() else {
        return;
    };

    let target = if spec.workload {
        Target::Workload
    } else {
        Target::document(
            spec.document_key.as_deref().unwrap_or("config.toml"),
            Format::from_annotation(spec.format.as_deref().unwrap_or("toml")),
        )
    };

    let images: Vec<&str> = spec.images.iter().map(String::as_str).collect();
    let images = if images.is_empty() {
        // The one input the grammar cannot usefully leave to chance: a stamp needs at least one
        // reader, and a fuzzer that never produces a well-formed digest would never reach the
        // properties below. A legal reference stands in, and the `i:` directive is still what
        // reaches the reference validator.
        vec![
            "ghcr.io/fuzz/app@sha256:0000000000000000000000000000000000000000000000000000000000000000",
        ]
    } else {
        images
    };

    // A refusal here is the *other* legitimate outcome, and the common one: most fuzzer-chosen
    // document keys and image references are illegal, and refusing them is the module working.
    // What must never happen is a stamp that is produced and is not legal.
    let Ok(metadata) = contract.kube_metadata(&target, &images) else {
        return;
    };

    every_emitted_key_and_value_is_kubernetes_legal(&metadata);
    the_stamp_is_accepted_by_its_own_checker(&contract, &target, &metadata);
    nothing_about_the_image_leaks_onto_the_object(&metadata, &prefix_of(&contract));
    the_yaml_block_is_stable_and_nests_where_it_is_told(&metadata);
}

/// The loader prefix this contract describes, which is the string most likely to leak.
fn prefix_of(contract: &Contract) -> String {
    contract.schema.dialect.prefix.clone()
}

/// Every key satisfies the Kubernetes key rule, and every label value the label-value rule.
fn every_emitted_key_and_value_is_kubernetes_legal(metadata: &Metadata) {
    for (key, value) in metadata.labels() {
        assert!(
            is_legal_key(key),
            "`{key}` is not a key Kubernetes accepts, so the whole object is refused"
        );
        assert!(
            is_legal_label_value(value),
            "`{key}` carries the label value `{value}`, which Kubernetes refuses — the object \
             cannot be applied at all"
        );
    }
    // Annotation *keys* obey the same rule. Annotation *values* are unconstrained, which is the
    // entire reason the image list and the document key live there rather than in labels, so
    // asserting a character class over them would be asserting the design away.
    for key in metadata.annotations().keys() {
        assert!(
            is_legal_key(key),
            "`{key}` is not a key Kubernetes accepts, so the whole object is refused"
        );
    }
}

/// What the stamp produced, the checker accepts.
fn the_stamp_is_accepted_by_its_own_checker(
    contract: &Contract,
    target: &Target,
    metadata: &Metadata,
) {
    if let Err(error) =
        contract.verify_kube_metadata(target, metadata.labels(), metadata.annotations())
    {
        panic!(
            "`kube_metadata` produced a stamp `verify_kube_metadata` refuses: {error}\n  \
             labels: {:?}\n  annotations: {:?}",
            metadata.labels(),
            metadata.annotations(),
        );
    }
}

/// The loader's prefix is a fact about an image, and must not appear on a Kubernetes object.
///
/// Not a restatement of the legality property: a short, tame prefix like `APP` is a perfectly
/// legal label value, so a change that started copying the prefix onto the object would pass
/// every check above and fail only for services whose prefix happens to be hostile. This asserts
/// the design decision directly.
fn nothing_about_the_image_leaks_onto_the_object(metadata: &Metadata, prefix: &str) {
    if prefix.is_empty() {
        return;
    }
    for (key, value) in metadata.labels().iter().chain(metadata.annotations()) {
        assert!(
            value != prefix,
            "`{key}` carries the loader's prefix `{prefix}`. That is a fact about the image, the \
             image already publishes it, and a second spelling here has nothing to catch it \
             drifting."
        );
    }
}

/// The rendering is byte-stable, and every line lands where the indent says it should.
fn the_yaml_block_is_stable_and_nests_where_it_is_told(metadata: &Metadata) {
    assert_eq!(
        metadata.to_yaml(2),
        metadata.to_yaml(2),
        "the same stamp rendered twice produced different bytes, so a committed block would \
         diff against itself"
    );

    for indent in [0, 2, 8] {
        let rendered = metadata.to_yaml(indent);
        for line in rendered.lines() {
            let depth = line.len() - line.trim_start().len();
            let header = line.trim_start().starts_with("labels:")
                || line.trim_start().starts_with("annotations:");
            let expected = if header { indent } else { indent + 2 };
            assert_eq!(
                depth, expected,
                "at indent {indent} the line `{line}` nests at {depth}, so pasting the block \
                 puts it under the wrong parent"
            );
        }
    }
}

/// The Kubernetes label-value rule, restated: 63 characters or fewer and, unless empty,
/// alphanumeric at both ends with `-`, `_` and `.` between.
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

/// The Kubernetes key rule, restated: an optional DNS-subdomain prefix of 253 characters or
/// fewer, a `/`, then a non-empty name of 63 characters or fewer under the label-value class.
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
