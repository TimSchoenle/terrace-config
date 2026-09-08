//! This renderer against the corpus, byte for byte.
//!
//! `spec/v1/conformance/<case>/rendered/` was blessed by the Rust implementation, from the types it
//! derived the document from. This crate renders the same bytes from the document alone. That the
//! two agree is the entire claim the shared toolchain rests on: a rendering is a pure function of
//! the published document, so it can be written once instead of once per producer.
//!
//! A failure here is one of two things, and the diff says which:
//!
//! - this renderer is wrong, which is the usual case and the reason the test exists;
//! - the Rust implementation changed a rendering, in which case its own
//!   `TERRACE_SPEC_BLESS` run moved the goldens and this is reporting that the change has not
//!   reached the shared renderer yet. Both halves have to move together, which is the point.
//!
//! There is deliberately no bless mode here. Two renderers that can each rewrite the expectation
//! are two renderers that agree by construction and prove nothing; the goldens have exactly one
//! author.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use terrace_contract::render::{self, Format, Options};
use terrace_contract::{Contract, DEFAULT_PATH, Tier};

/// Every stored case, by directory name.
const CASES: &[&str] = &["minimal", "full-surface", "unnameable-key"];

/// The file each format is stored under.
const RENDERINGS: &[(Format, &str)] = &[
    (Format::Markdown, "markdown.md"),
    (Format::MarkdownLoader, "markdown-loader.md"),
    (Format::MarkdownKeys, "markdown-keys.md"),
    (Format::Toml, "config.toml"),
    (Format::JsonSchema, "schema.json"),
    (Format::Labels, "labels.txt"),
    (Format::Dockerfile, "Dockerfile.part"),
];

/// `cli/` is a sibling of `spec/`, not a child of it.
fn spec_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate has a parent directory")
        .join("spec")
        .join("v1")
}

fn case_dir(case: &str) -> PathBuf {
    spec_dir().join("conformance").join(case)
}

fn contract(case: &str) -> Contract {
    let path = case_dir(case).join("contract.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()));
    Contract::from_json(&text)
        .unwrap_or_else(|e| panic!("{} is not a readable contract: {e}", path.display()))
}

/// The stored bytes, with line endings normalised.
///
/// `.gitattributes` checks `spec/**` out as LF precisely so the corpus is byte-comparable, but a
/// working tree that predates that rule, or a checkout through a tool that ignores it, would fail
/// every case here for a reason that has nothing to do with the rendering.
fn golden(case: &str, file: &str) -> Option<String> {
    let path = case_dir(case).join("rendered").join(file);
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.replace("\r\n", "\n"))
}

#[test]
fn every_rendering_matches_the_corpus() {
    let mut stale: BTreeMap<String, String> = BTreeMap::new();

    for case in CASES {
        let contract = contract(case);
        let options = Options {
            path: DEFAULT_PATH,
            ..Options::default()
        };

        for (format, file) in RENDERINGS {
            let key = format!("{case}/{file}");
            let found = render::one_newline(
                render::render(&contract, *format, &options).expect("the rendering succeeds"),
            );

            match golden(case, file) {
                Some(expected) if expected == found => {}
                Some(expected) => {
                    stale.insert(key, first_difference(&found, &expected));
                }
                None => {
                    stale.insert(key, "the golden is missing".to_owned());
                }
            }
        }
    }

    let mut report = String::new();
    for (name, detail) in &stale {
        let _ = write!(report, "\n`{name}`: {detail}");
    }

    assert!(
        stale.is_empty(),
        "this renderer disagrees with the corpus. The goldens have one author — the Rust \
         implementation's `TERRACE_SPEC_BLESS` run — so either this renderer is wrong, or a \
         rendering changed there and has not reached here.\n{report}"
    );
}

/// `--format contract` round-trips a document byte for byte.
///
/// Not a rendering so much as a property of the model: a document read and written again must be
/// the document that was read. Without it `stamp` cannot exist, because stamping a build identity
/// onto a contract would silently reorder or drop every field this build does not model.
#[test]
fn the_document_survives_a_round_trip() {
    for case in CASES {
        let path = case_dir(case).join("contract.json");
        let stored = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
            .replace("\r\n", "\n");

        let written = render::one_newline(
            render::render(&contract(case), Format::Contract, &Options::default())
                .expect("the contract renders"),
        );

        assert_eq!(
            written,
            stored,
            "`{case}` does not survive a read and a write. {}",
            first_difference(&written, &stored)
        );
    }
}

/// The `json` rendering is the schema half of the stored document, unchanged.
#[test]
fn the_schema_half_is_the_stored_one() {
    for case in CASES {
        let path = case_dir(case).join("contract.json");
        let stored: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("the stored contract can be read"),
        )
        .expect("the stored contract is JSON");

        let rendered: serde_json::Value = serde_json::from_str(
            &render::render(&contract(case), Format::Json, &Options::default())
                .expect("the schema renders"),
        )
        .expect("the rendered schema is JSON");

        assert_eq!(rendered, stored["schema"], "`{case}`");
    }
}

/// The `config.example.toml` rendering parses.
///
/// The goldens pin what it *says*; this pins that what it says loads. An example file is copied
/// into a deployment rather than read and closed, so one that fails to parse is a defect that
/// reaches production through the one artefact nobody validates.
#[test]
fn the_example_file_loads() {
    for case in CASES {
        let rendered = render::render(&contract(case), Format::Toml, &Options::default())
            .expect("the example renders");
        toml::from_str::<toml::Value>(&rendered).unwrap_or_else(|e| {
            panic!(
                "`{case}`'s example does not parse: {e}
{rendered}"
            )
        });
    }
}

/// The first line the two differ on, which is what a reader needs and what a dump buries.
fn first_difference(found: &str, expected: &str) -> String {
    for (line, (a, b)) in found.lines().zip(expected.lines()).enumerate() {
        if a != b {
            return format!("line {}: rendered `{a}`, stored `{b}`", line + 1);
        }
    }
    format!(
        "rendered {} lines, stored {}",
        found.lines().count(),
        expected.lines().count()
    )
}

/// Every stored case conforms at tier 2.
///
/// The Rust implementation claims tier 3 against itself, and tier 3 includes tier 2 — so a corpus
/// case failing here is either a defect in `conform` or a document the reference producer should
/// never have emitted. Both are worth failing a build for, and this is the only test that would
/// catch the second.
#[test]
fn the_corpus_conforms_at_the_dialect_tier() {
    for case in CASES {
        let violations = terrace_contract::conform::conform(&contract(case), Tier::Dialect);
        assert!(
            violations.is_empty(),
            "`{case}` does not conform at tier 2: {}",
            violations
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
}

/// Every stored case satisfies the meta-schema this binary embeds.
///
/// The corpus is checked against `contract.schema.json` on the producer's side too. This is the
/// same check from the consumer's, and it is what says the *embedded* copy is the published one —
/// a binary shipping a stale meta-schema would validate documents against a specification nobody
/// else is holding them to.
#[test]
fn the_corpus_satisfies_the_embedded_meta_schema() {
    for case in CASES {
        let text = std::fs::read_to_string(case_dir(case).join("contract.json"))
            .expect("the stored contract can be read");
        let errors = terrace_contract::validate::validate(&text).expect("the meta-schema compiles");
        assert!(errors.is_empty(), "`{case}`: {}", errors.join("; "));
    }
}
