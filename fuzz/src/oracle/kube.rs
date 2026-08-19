//! Is a stamp this crate produced one Kubernetes would actually accept?
//!
//! The other oracles ask whether the loader does what it says. This one asks something narrower and
//! sharper: **for any contract the builder accepts, is every key and value `kube_metadata` emits
//! legal Kubernetes metadata** — and does the check that reads it accept what the generator wrote.
//!
//! That property is worth fuzzing rather than exampling because of where a violation surfaces. An
//! illegal label value is not a failed check; the API server refuses the object at `kubectl apply`,
//! so a stamp this crate got wrong does not weaken a gate, it stops a chart rolling out — with a
//! message naming the object and nothing naming the crate that produced the value. The failure
//! lands on an operator at deploy time, days after the build that caused it.
//!
//! And the inputs that reach it are ordinary. A dialect prefix *always* ends in a separator, so
//! every contract this crate can build carries at least one string that is illegal as a label
//! value; the module's whole job is to keep such strings out of labels. A fuzzer choosing the
//! prefix, the app name, the document key and the format token is exercising precisely the seam
//! where one could leak through.
//!
//! The rules are restated in [`legal`] rather than called out of the crate. An oracle that asked
//! the code under test whether its own output was legal would agree with it by construction, which
//! is the one thing this must not do.
//!
//! # The input
//!
//! ```text
//! p:<prefix>      the dialect prefix, default `TEST_`
//! n:<name>        the app name
//! k:<a>/<b>       declare a leaf at `a.b`
//! d:<key>         the document key
//! f:<format>      the document format token
//! i:<reference>   an image that reads the document, verbatim
//! w               stamp a workload rather than a document
//! ```
//!
//! Every one of those is passed through untouched. A refusal is a legitimate outcome and not a
//! finding — an unpinned reference and a document key with a `/` in it are both things the module
//! exists to refuse — so the properties below are asserted only about a stamp that was *produced*.

use std::collections::BTreeMap;

use terrace_config::Terrace;
use terrace_config::schema::kube::{
    ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT, ANNOTATION_IMAGES, Format, LABEL_CONTRACT_VERSION,
    Metadata, Pairing, Target,
};
use terrace_config::schema::{App, Contract, DEFAULT_PATH, Describe, Leaf, Sink};

use crate::support::{MAX_DIRECTIVES, MAX_NAME_LEN, PREFIX};

/// The most leaves one iteration will describe.
const MAX_LEAVES: usize = 8;

/// The most images one iteration will name, so a stamp stays a stamp rather than a disk-fill.
const MAX_IMAGES: usize = 8;

/// The indents a pasted block is checked at: none, a `metadata:` block, a pod template's.
const INDENTS: [usize; 3] = [0, 2, 8];

/// A reference that is digest-pinned and is never one the input can name.
///
/// Used for the negative half of the membership check. The digest is all `b`s, which no seed
/// spells and a mutation engine will not stumble onto.
const OUTSIDER: &str = "ghcr.io/oracle/outsider@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// What the input asked for.
#[derive(Debug, Default)]
struct Spec {
    prefix: Option<String>,
    app: Option<String>,
    leaves: Vec<Vec<String>>,
    document_key: Option<String>,
    format: Option<String>,
    images: Vec<String>,
    workload: bool,
}

thread_local! {
    /// The leaves the current iteration describes.
    ///
    /// [`Describe::describe`] takes no value, so a fuzzer-driven implementation has nowhere else to
    /// read its input from. Thread local because the replay suite runs oracles in parallel.
    static LEAVES: std::cell::RefCell<Vec<Vec<String>>> = const { std::cell::RefCell::new(Vec::new()) };
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
        match kind {
            "p" if rest.len() <= MAX_NAME_LEN => spec.prefix = Some(rest.to_owned()),
            "n" if rest.len() <= MAX_NAME_LEN => spec.app = Some(rest.to_owned()),
            "d" if rest.len() <= MAX_NAME_LEN => spec.document_key = Some(rest.to_owned()),
            "f" if rest.len() <= MAX_NAME_LEN => spec.format = Some(rest.to_owned()),
            "i" if !rest.is_empty() && rest.len() <= MAX_NAME_LEN => {
                if spec.images.len() < MAX_IMAGES {
                    spec.images.push(rest.to_owned());
                }
            }
            "k" => {
                let segments: Vec<String> = rest
                    .split('/')
                    .filter(|segment| !segment.is_empty())
                    .take(4)
                    .map(ToOwned::to_owned)
                    .collect();
                // A duplicate path is what `Sink::leaf` panics on by design, so feeding it would
                // be fuzzing the assertion rather than the code under it.
                let joined = segments.join(".");
                if !segments.is_empty()
                    && spec.leaves.len() < MAX_LEAVES
                    && segments.iter().all(|s| s.len() <= MAX_NAME_LEN)
                    && !seen.contains(&joined)
                {
                    seen.push(joined);
                    spec.leaves.push(segments);
                }
            }
            _ => {}
        }
    }
    spec
}

/// The contract the input describes, or [`None`] if the builder refuses it.
///
/// A refusal is not a finding. An empty prefix is refused by design — a prefixless loader cannot
/// tell its own namespace from the machine's — and everything downstream of this point is a claim
/// about contracts that were *built*.
fn contract(spec: &Spec) -> Option<Contract> {
    let prefix = spec.prefix.as_deref().unwrap_or(PREFIX);
    let schema = LEAVES.with(|cell| {
        cell.replace(spec.leaves.clone());
        let schema = Terrace::new(prefix).schema::<Fuzzed>();
        cell.replace(Vec::new());
        schema
    });
    let app = App::new(spec.app.clone().unwrap_or_else(|| "fuzzed".to_owned()));
    schema.into_contract(app).build().ok()
}

/// The target the input describes.
fn target(spec: &Spec) -> Target {
    if spec.workload {
        return Target::Workload;
    }
    Target::document(
        spec.document_key
            .clone()
            .unwrap_or_else(|| "config.toml".to_owned()),
        Format::from(spec.format.as_deref().unwrap_or("toml")),
    )
}

/// Run every property over one input.
pub fn check(data: &str) {
    let spec = parse(data);
    let Some(contract) = contract(&spec) else {
        return;
    };
    let target = target(&spec);

    let images: Vec<&str> = spec.images.iter().map(String::as_str).collect();
    // No images at all is refused by design, so the default keeps most iterations productive
    // rather than bouncing off the same guard. An `i:` directive replaces it entirely.
    let images: Vec<&str> = if images.is_empty() {
        vec![OUTSIDER]
    } else {
        images
    };

    // A refusal is the other legitimate outcome: an unpinned reference, a repeat, a document key
    // with a `/` in it. What must never happen is a stamp that was produced and is illegal.
    let Ok(metadata) = contract.kube_metadata(&target, &images) else {
        return;
    };

    everything_emitted_is_legal_kubernetes_metadata(&metadata);
    the_stamp_carries_exactly_what_its_target_defines(&metadata, &target);
    the_check_that_reads_it_accepts_what_the_generator_wrote(&contract, &target, &metadata);
    the_pasted_block_is_stable_and_well_formed(&metadata);
    // Document targets only. `Pairing` reads the document key and the format, which a workload
    // stamp deliberately does not carry — a pod is checked with `verify_kube_metadata`, above.
    // The first sweep of this oracle ran the pairing over both and reported the missing key as a
    // finding, which is the oracle modelling the API wrongly rather than the API being wrong.
    if matches!(target, Target::Document { .. }) {
        every_named_image_pairs_and_no_other_does(&contract, &metadata, &images);
    }
}

/// Every key is a legal Kubernetes key, and every *label* value a legal label value.
///
/// Annotation values are deliberately not checked against the label rule: they are unconstrained,
/// and the whole design rests on that being where the unconstrained things go. What is checked is
/// that nothing which fails the label rule ended up in a label.
fn everything_emitted_is_legal_kubernetes_metadata(metadata: &Metadata) {
    for (key, value) in metadata.labels() {
        assert!(
            legal::key(key),
            "`{key}` is not a legal Kubernetes label key"
        );
        assert!(
            legal::label_value(value),
            "the label `{key}` carries `{value}`, which Kubernetes refuses as a label value"
        );
    }
    for key in metadata.annotations().keys() {
        assert!(
            legal::key(key),
            "`{key}` is not a legal Kubernetes annotation key"
        );
    }
    // The map as a whole is bounded at 256 KiB, and the image list is the only value that grows.
    let size: usize = metadata
        .annotations()
        .iter()
        .map(|(key, value)| key.len() + value.len())
        .sum();
    assert!(size <= 256 * 1024, "the annotations exceed 256 KiB: {size}");
}

/// A document names its entry and its parser; a workload names neither.
fn the_stamp_carries_exactly_what_its_target_defines(metadata: &Metadata, target: &Target) {
    assert!(
        metadata.labels().contains_key(LABEL_CONTRACT_VERSION),
        "a stamp with no version label is one no selector can find"
    );
    assert!(
        metadata.annotations().contains_key(ANNOTATION_IMAGES),
        "a stamp with no image list is one no pairing can satisfy"
    );

    let document = matches!(target, Target::Document { .. });
    for name in [ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT] {
        assert_eq!(
            metadata.annotations().contains_key(name),
            document,
            "`{name}` describes a document, and this stamp is for {}",
            if document { "one" } else { "a pod" }
        );
    }
}

/// What the generator wrote, the checker must accept.
fn the_check_that_reads_it_accepts_what_the_generator_wrote(
    contract: &Contract,
    target: &Target,
    metadata: &Metadata,
) {
    if let Err(error) =
        contract.verify_kube_metadata(target, metadata.labels(), metadata.annotations())
    {
        panic!("`kube_metadata` produced a stamp `verify_kube_metadata` refuses: {error}");
    }

    // Extra keys are ignored, which is the rule that lets a chart add its own labels. Asserted
    // here because a checker that quietly tightened it would break every real chart at once.
    let mut labels = metadata.labels().clone();
    labels.insert("app.kubernetes.io/name".to_owned(), "fuzzed".to_owned());
    let mut annotations = metadata.annotations().clone();
    annotations.insert("example.com/unrelated".to_owned(), String::new());
    if let Err(error) = contract.verify_kube_metadata(target, &labels, &annotations) {
        panic!("a chart's own labels are not this crate's business, but were refused: {error}");
    }
}

/// The pasted block is byte-stable, and indents where it was asked to.
fn the_pasted_block_is_stable_and_well_formed(metadata: &Metadata) {
    for indent in INDENTS {
        let rendered = metadata.to_yaml(indent);
        assert_eq!(
            rendered,
            metadata.to_yaml(indent),
            "the block a chart commits is not byte-stable"
        );
        assert!(!rendered.contains('\t'), "a tab is not YAML indentation");
        assert!(rendered.ends_with('\n'), "the block does not end a line");

        let outer = " ".repeat(indent);
        let inner = " ".repeat(indent + 2);
        for line in rendered.lines() {
            assert!(!line.trim().is_empty(), "a blank line in `{rendered}`");
            if line.ends_with(':') {
                assert!(
                    line.starts_with(&outer) && !line[indent..].starts_with(' '),
                    "a block key is not at column {indent}: `{line}`"
                );
            } else {
                assert!(
                    line.starts_with(&inner) && !line[indent + 2..].starts_with(' '),
                    "an entry is not at column {}: `{line}`",
                    indent + 2
                );
                // A value must be quoted, or `"1"` would parse as the integer 1 — which is not a
                // thing a Kubernetes label value may be.
                let value = line.trim_start().split_once(": ").map(|(_, value)| value);
                assert!(
                    value.is_some_and(|value| value.starts_with('"') && value.ends_with('"')),
                    "an entry's value is not quoted: `{line}`"
                );
            }
        }
    }
}

/// Every image the stamp names pairs with it, and one it does not name does not.
///
/// Both halves matter. A pairing that accepted any image with a valid stamp would report that a
/// `ConfigMap` and a pod agree whenever both were stamped at all, which is the check failing open.
fn every_named_image_pairs_and_no_other_does(
    contract: &Contract,
    metadata: &Metadata,
    images: &[&str],
) {
    let labels: BTreeMap<String, String> = contract
        .labels(DEFAULT_PATH)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();

    for running in images {
        if let Err(error) = Pairing::new(contract, DEFAULT_PATH)
            .image(running, &labels)
            .object(metadata.labels(), metadata.annotations())
            .check()
        {
            panic!("`{running}` is named by this stamp and did not pair with it: {error}");
        }
    }

    // Skipped when the input happened to name the outsider itself, which the `i:` directive can do.
    if !images.contains(&OUTSIDER) {
        assert!(
            Pairing::new(contract, DEFAULT_PATH)
                .image(OUTSIDER, &labels)
                .object(metadata.labels(), metadata.annotations())
                .check()
                .is_err(),
            "an image this document was not rendered for paired with it anyway"
        );
    }
}

/// The Kubernetes rules, restated.
///
/// Restated and not called: the crate's own predicates are what is under test, and an oracle that
/// asked them whether their output was legal would agree with them by construction — including
/// about a rule they had both got wrong. These are written from the API server's documented
/// grammar instead.
mod legal {
    /// A label value: at most 63 characters, beginning and ending alphanumeric, with `-`, `_` and
    /// `.` between. The empty string is legal.
    pub fn label_value(value: &str) -> bool {
        if value.is_empty() {
            return true;
        }
        if value.len() > 63 {
            return false;
        }
        let bytes = value.as_bytes();
        bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_' || *b == b'.')
            && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
            && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
    }

    /// A label or annotation key: an optional DNS subdomain of at most 253 characters, a `/`, then
    /// a non-empty name of at most 63 under the label-value rule.
    pub fn key(key: &str) -> bool {
        let (prefix, name) = match key.split_once('/') {
            Some((prefix, name)) => (Some(prefix), name),
            None => (None, key),
        };
        if let Some(prefix) = prefix
            && (prefix.is_empty() || prefix.len() > 253 || !prefix.split('.').all(dns_label))
        {
            return false;
        }
        !name.is_empty() && label_value(name)
    }

    /// One dot-separated segment of a DNS subdomain.
    fn dns_label(label: &str) -> bool {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && label.bytes().next().is_some_and(|b| b != b'-')
            && label.bytes().next_back().is_some_and(|b| b != b'-')
    }
}
