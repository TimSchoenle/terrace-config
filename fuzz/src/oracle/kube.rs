//! Is every byte the Kubernetes stamp emits one the API server would actually hold?
//!
//! The `schema` oracle fuzzes a claim made about the loader. This one fuzzes a claim made about a
//! *platform*, and the failure mode is worse than either: a document with a bad field is caught by
//! whoever parses it, whereas a label value Kubernetes refuses is caught by `kubectl apply` — in a
//! chart repository, days after the crate emitted it, by somebody with no reason to look here.
//!
//! Two properties, over any contract [`ContractBuilder::build`] accepts:
//!
//! 1. **Legality.** Every key and value
//!    [`Contract::kube_metadata`](terrace_config::schema::Contract::kube_metadata) emits satisfies
//!    the platform's own character rules — which are restated in [`legal`] rather than reached
//!    through the crate's validators, because an oracle that asked the code under test which values
//!    are legal would agree with it by construction.
//! 2. **Round trip.** What `kube_metadata` produced,
//!    [`verify_kube_metadata`](terrace_config::schema::Contract::verify_kube_metadata) accepts —
//!    and a [`Pairing`] against the image labels the same contract publishes holds for every image
//!    the stamp lists.
//!
//! A *refusal* is never a finding. The crate is entitled to refuse a document key, a format or a
//! reference the input made up; what it is not entitled to do is emit one and have the platform
//! refuse it later.
//!
//! # The input
//!
//! ```text
//! p:<prefix>      the loader's environment prefix, default `TEST_`
//! n:<name>        the app name
//! k:<a>/<b>/<c>   declare a leaf at `a.b.c`
//! d:<key>         the key of the `data` entry holding the document
//! f:<syntax>      the document's syntax
//! i:<reference>   an image that reads the document, repeatable
//! ```
//!
//! The prefix and the app name are in the grammar although neither reaches the stamp. That is the
//! point: they are the two contract fields most hostile to the label rules — a prefix ends in a
//! separator, an app name is prose — and a target that could not vary them could not notice one of
//! them starting to leak.

use std::collections::{BTreeMap, BTreeSet};

use terrace_config::Terrace;
use terrace_config::schema::kube::{
    ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT, ANNOTATION_IMAGES, Format, Metadata, Pairing,
    Target,
};
use terrace_config::schema::{App, Contract, DEFAULT_PATH, Describe, Leaf, Sink};

use crate::support::{MAX_DIRECTIVES, MAX_NAME_LEN, PREFIX};

/// The most leaves one iteration will describe, and the deepest a path will nest.
const MAX_LEAVES: usize = 8;
const MAX_DEPTH: usize = 4;

/// The most images one stamp will list. An unbounded list is an unbounded string to build and
/// nothing further to learn: the membership rule is the same at three as at three hundred.
const MAX_IMAGES: usize = 8;

/// The longest reference one `i:` directive will carry.
///
/// Its own bound rather than [`MAX_NAME_LEN`]: a digest alone is 71 characters, so the file-name
/// bound the other oracles share would silently drop every pinned reference with a registry, a port
/// and a tag in front of it — which is the shape a real one has.
const MAX_REFERENCE_LEN: usize = 160;

/// The indents `to_yaml` is asked for. `0` is the block on its own, `2` is a `metadata:` child,
/// and `8` is four levels into a Helm template — the depth at which an off-by-one first shows.
const INDENTS: [usize; 3] = [0, 2, 8];

/// What the input asked for.
#[derive(Debug, Default)]
struct Spec {
    prefix: Option<String>,
    app: Option<String>,
    leaves: Vec<Vec<String>>,
    document_key: Option<String>,
    format: Option<String>,
    images: Vec<String>,
}

thread_local! {
    /// The leaves the current iteration describes.
    ///
    /// [`Describe::describe`] takes no value, so a fuzzer-driven implementation has nowhere else to
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
        [] => unreachable!("a leaf always has at least one segment"),
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
    // Keyed on the joined path, which is what `Sink` keys on: two spellings of one key path make
    // `Sink::leaf` panic by design, so feeding it a pair would be fuzzing the assertion.
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for line in data.lines().take(MAX_DIRECTIVES) {
        let Some((kind, rest)) = line.split_once(':') else {
            continue;
        };
        match kind {
            "p" if rest.len() <= MAX_NAME_LEN => spec.prefix = Some(rest.to_owned()),
            "n" if rest.len() <= MAX_NAME_LEN => spec.app = Some(rest.to_owned()),
            "d" if rest.len() <= MAX_NAME_LEN => spec.document_key = Some(rest.to_owned()),
            "f" if rest.len() <= MAX_NAME_LEN => spec.format = Some(rest.to_owned()),
            "i" if rest.len() <= MAX_REFERENCE_LEN && spec.images.len() < MAX_IMAGES => {
                spec.images.push(rest.to_owned());
            }
            "k" if spec.leaves.len() < MAX_LEAVES => {
                let segments: Vec<String> = rest
                    .split('/')
                    .take(MAX_DEPTH)
                    .filter(|segment| !segment.is_empty() && segment.len() <= MAX_NAME_LEN)
                    .map(ToOwned::to_owned)
                    .collect();
                if !segments.is_empty() && seen.insert(segments.join(".")) {
                    spec.leaves.push(segments);
                }
            }
            _ => {}
        }
    }
    spec
}

/// Run every property over one input.
pub fn check(data: &str) {
    let spec = parse(data);
    if spec.leaves.is_empty() || spec.images.is_empty() {
        return;
    }

    let terrace = Terrace::new(spec.prefix.as_deref().unwrap_or(PREFIX));
    let schema = LEAVES.with(|cell| {
        cell.replace(spec.leaves.clone());
        let schema = terrace.schema::<Fuzzed>();
        cell.replace(Vec::new());
        schema
    });
    // A contract the builder refuses is not this oracle's business — the `schema` half already
    // fuzzes what `build` accepts, and there is no stamp to be legal or illegal without one.
    let Ok(contract) = schema
        .into_contract(App::new(spec.app.clone().unwrap_or_default()))
        .build()
    else {
        return;
    };

    let images: Vec<&str> = spec.images.iter().map(String::as_str).collect();
    let document = Target::document(
        spec.document_key.clone().unwrap_or_default(),
        Format::parse(spec.format.as_deref().unwrap_or_default()),
    );

    for target in [document, Target::Workload] {
        // A refusal is always a legitimate answer: the input names the document key, the format and
        // every reference, and none of them has to be one a stamp can carry.
        let Ok(metadata) = contract.kube_metadata(&target, &images) else {
            continue;
        };

        every_entry_is_one_kubernetes_would_hold(&metadata);
        the_target_decides_which_entries_exist(&target, &metadata);
        the_stamp_is_the_same_bytes_twice(&contract, &target, &images, &metadata);
        the_block_a_chart_pastes_is_a_mapping(&metadata);
        what_it_emitted_it_accepts(&contract, &target, &metadata, &images);
    }
}

/// Property 1: the platform's own character rules, over every key and every label value.
fn every_entry_is_one_kubernetes_would_hold(metadata: &Metadata) {
    for (key, value) in metadata.labels() {
        assert!(
            legal::metadata_key(key),
            "`{key}` is not a key Kubernetes accepts, and it was emitted as a label"
        );
        assert!(
            legal::label_value(value),
            "`{key}` was emitted carrying `{value}`, which is not a label value: the API server \
             refuses this object on apply"
        );
    }
    // Annotation *values* are unconstrained by the platform, which is the whole reason the image
    // list is an annotation. Only the keys are held to a rule.
    for key in metadata.annotations().keys() {
        assert!(
            legal::metadata_key(key),
            "`{key}` is not a key Kubernetes accepts, and it was emitted as an annotation"
        );
    }
}

/// Property 1b: a pod is not a document, so it carries neither of a document's two annotations.
fn the_target_decides_which_entries_exist(target: &Target, metadata: &Metadata) {
    let annotations = metadata.annotations();
    assert!(
        annotations.contains_key(ANNOTATION_IMAGES),
        "every stamp names the images that read the document"
    );

    let document = matches!(target, Target::Document { .. });
    for name in [ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT] {
        assert_eq!(
            annotations.contains_key(name),
            document,
            "`{name}` is a property of a document, and this stamp is for {target:?}"
        );
    }
}

/// Property 1c: the same inputs produce the same bytes, which is what lets a manifest be diffed.
fn the_stamp_is_the_same_bytes_twice(
    contract: &Contract,
    target: &Target,
    images: &[&str],
    metadata: &Metadata,
) {
    let again = contract
        .kube_metadata(target, images)
        .expect("what it accepted once it accepts twice");
    assert_eq!(metadata.labels(), again.labels(), "the labels moved");
    assert_eq!(
        metadata.annotations(),
        again.annotations(),
        "the annotations moved"
    );
    for indent in INDENTS {
        assert_eq!(
            metadata.to_yaml(indent),
            again.to_yaml(indent),
            "the pasted block moved at indent {indent}"
        );
    }
}

/// Property 1d: the pasted block is a YAML mapping of mappings, at every indent it is asked for.
///
/// Checked by looking rather than by parsing: pulling a YAML crate in to assert about six lines
/// would be a dependency this repository would then have to argue for, and what matters is the
/// shape a parser would find — one heading per map, every entry two columns deeper, both halves
/// quoted so that `"1"` stays a string.
fn the_block_a_chart_pastes_is_a_mapping(metadata: &Metadata) {
    for indent in INDENTS {
        let rendered = metadata.to_yaml(indent);
        assert!(
            rendered.ends_with('\n'),
            "the block does not end a line, so whatever follows it in the template joins it:\n\
             {rendered}"
        );
        for line in rendered.lines() {
            let depth = line.len() - line.trim_start_matches(' ').len();
            let body = line.trim_start_matches(' ');
            assert!(
                !body.is_empty(),
                "a blank line breaks the block:\n{rendered}"
            );
            assert!(
                !body.contains('\n') && !body.contains('\r'),
                "a value carried a line ending out of its own line:\n{rendered}"
            );

            if depth == indent {
                assert!(
                    body == "labels:" || body == "annotations:",
                    "`{body}` is not a heading this block emits:\n{rendered}"
                );
            } else {
                assert_eq!(
                    depth,
                    indent + 2,
                    "`{body}` sits at neither depth this block uses:\n{rendered}"
                );
                assert!(
                    body.starts_with('"') && body.ends_with('"') && body.contains("\": \""),
                    "`{body}` is not a quoted `key: value` pair, so the API server would read \
                     something other than what was emitted:\n{rendered}"
                );
            }
        }
    }
}

/// Property 2: what it emitted, it accepts — and pairs with the image labels of the same contract.
fn what_it_emitted_it_accepts(
    contract: &Contract,
    target: &Target,
    metadata: &Metadata,
    images: &[&str],
) {
    contract
        .verify_kube_metadata(target, metadata.labels(), metadata.annotations())
        .unwrap_or_else(|error| {
            panic!(
                "a stamp this contract produced is one it refuses: {error}\n  labels: {:?}\n  \
                 annotations: {:?}",
                metadata.labels(),
                metadata.annotations()
            )
        });

    let image_labels: BTreeMap<String, String> = contract
        .labels(DEFAULT_PATH)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    for running in images {
        Pairing::new(contract)
            .image(running, DEFAULT_PATH, &image_labels)
            .object(target, metadata.labels(), metadata.annotations())
            .check()
            .unwrap_or_else(|error| {
                panic!(
                    "`{running}` is one of the images this stamp lists, and the pairing refuses \
                     it: {error}"
                )
            });
    }
}

/// The platform's rules, restated.
///
/// Deliberately a second implementation. The crate has its own, and an oracle that called it would
/// be asserting that the code agrees with itself — which it does, including about any value it is
/// wrong about.
mod legal {
    /// Whether Kubernetes would hold `value` as a label value.
    pub fn label_value(value: &str) -> bool {
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
    pub fn metadata_key(key: &str) -> bool {
        let Some((prefix, name)) = key.split_once('/') else {
            return !key.is_empty() && label_value(key);
        };
        if name.is_empty() || name.contains('/') || !label_value(name) {
            return false;
        }
        !prefix.is_empty()
            && prefix.len() <= 253
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
}
