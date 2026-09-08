//! Every rendering, against the claim each one makes about its own output.
//!
//! A renderer that panics is a build that fails, which is bad and obvious. The findings worth
//! hunting here are the quiet ones: output that is *well-formed enough to ship* and wrong.
//!
//! So each rendering is held to the promise it makes rather than to a golden file. The corpus
//! tests already pin what these produce for three documents; this asks what they produce for
//! documents nobody wrote, which is where a key name carrying a quote, a default carrying a
//! newline, or a path that is only separators gets in.

use serde_json::Value as Json;
use terrace_contract::Contract;
use terrace_contract::render::{self, Format, Options};

use crate::mutate;

/// Run every rendering over one mutated document.
///
/// # Panics
/// On any finding.
pub fn check(data: &str) {
    let document = mutate::render(&mutate::document(data));
    let Ok(contract) = Contract::from_json(&document) else {
        return;
    };

    let options = Options::default();
    for format in Format::ALL {
        let rendered = render::render(&contract, *format, &options)
            .expect("every rendering of a readable document succeeds");

        match format {
            // The TOML rendering's whole claim is that the file it writes loads. An example file is
            // *copied* into a deployment rather than read and closed, so one that fails to parse is
            // a defect reaching production through the one artefact nobody validates.
            Format::Toml => {
                toml::from_str::<toml::Value>(&rendered).unwrap_or_else(|e| {
                    panic!("the rendered example does not parse: {e}\n{rendered}")
                });
            }
            // Three renderings claim to be JSON, and one of them claims more: a JSON Schema whose
            // `properties` is not an object is a document every validator rejects at load, which
            // turns a configuration gate into a broken pipeline.
            Format::Json | Format::Contract => {
                serde_json::from_str::<Json>(&rendered).expect("a JSON rendering is JSON");
            }
            Format::JsonSchema => {
                let schema: Json =
                    serde_json::from_str(&rendered).expect("the JSON Schema rendering is JSON");
                assert!(
                    schema.get("properties").is_some_and(Json::is_object),
                    "a JSON Schema whose `properties` is not an object is rejected at load"
                );
            }
            Format::Markdown | Format::MarkdownLoader | Format::MarkdownKeys => {
                check_table(&rendered);
            }
            // A `LABEL` instruction that breaks across a line stops being one instruction, and the
            // image builds without the label rather than failing.
            Format::Labels => assert!(
                !rendered.contains('\n') || rendered.lines().count() == 3,
                "the labels are one `NAME=value` per label:\n{rendered}"
            ),
            Format::Dockerfile => {
                assert!(
                    rendered.starts_with("# terrace-config:labels:begin"),
                    "the block must open with the marker a drift check looks for"
                );
                assert!(
                    rendered.trim_end().ends_with("# terrace-config:labels:end"),
                    "an unterminated block is one a drift check reads past"
                );
                let committed = render::image::committed_block(&rendered)
                    .expect("a block this crate rendered is one it can read back");
                assert!(
                    committed.contains("LABEL "),
                    "the region between the markers must carry the instruction"
                );
            }
            // `Format` is `#[non_exhaustive]`, so a rendering added later lands here rather than
            // failing to compile. Silently passing it would be worse than either: the point of
            // this oracle is that every rendering has a claim it is held to, and one arriving
            // without one should stop a build rather than slip through unchecked.
            other => panic!(
                "`{other}` has no oracle. Every rendering makes a claim about its own output; add the one it makes before shipping it."
            ),
        }
    }

    // The two spellings of one rendering must not disagree. `markdown` is the pair; asking for
    // either half alone has to produce the same bytes as that half of the pair, or a page built
    // from the halves says something different from a page built from the whole.
    let both = render::render(&contract, Format::Markdown, &options).expect("renders");
    let loader = render::render(&contract, Format::MarkdownLoader, &options).expect("renders");
    let keys = render::render(&contract, Format::MarkdownKeys, &options).expect("renders");
    if loader.is_empty() {
        assert_eq!(
            both, keys,
            "with no loader table, the pair is the key table"
        );
    } else {
        assert_eq!(
            both,
            format!("{loader}\n{keys}"),
            "the pair is not its two halves"
        );
    }

    check_placeholders(&contract);
}

/// Every row of a GitHub-flavoured table has the header's column count.
///
/// The failure this catches is the one a person reading the diff does not: an unescaped `|` in a
/// doc comment or a key name adds a column, and the renderer emits a table that *looks* fine and
/// puts every cell after it under the wrong heading.
fn check_table(rendered: &str) {
    // Per table, not per rendering: `markdown` is the loader table *and* the key table, separated
    // by a blank line, and the two have different widths on purpose. A blank line ends a table in
    // GitHub-flavoured Markdown, which is why the pair needs one between them at all.
    let mut expected = None;
    for line in rendered.lines() {
        if line.trim().is_empty() {
            expected = None;
            continue;
        }
        if !line.starts_with('|') {
            continue;
        }
        let columns = count_cells(line);
        match expected {
            None => expected = Some(columns),
            Some(width) => assert_eq!(
                columns, width,
                "a row has {columns} cells and the header has {width}; an unescaped separator \
                 moves every cell after it under the wrong heading\n{rendered}"
            ),
        }
    }
}

/// Cells in one row, counting an escaped `\|` as content rather than as a separator.
fn count_cells(line: &str) -> usize {
    let mut cells = 0;
    let mut escaped = false;
    for character in line.chars() {
        match character {
            '\\' if !escaped => escaped = true,
            '|' if !escaped => cells += 1,
            _ => escaped = false,
        }
    }
    cells
}

/// The TOML rendering's placeholder is the shape the document published, not a guess at it.
///
/// The property that makes this renderer language-agnostic, asserted rather than assumed: a key
/// whose `constraint` says `integer` gets a bare `0`, and one whose constraint says nothing gets a
/// quoted string. A regression to reading `ty` would pass every corpus case — all three are Rust
/// documents — and fail here the moment a mutation writes a Java type name.
fn check_placeholders(contract: &Contract) {
    let rendered = render::render(contract, Format::Toml, &Options::default()).expect("renders");

    for key in &contract.schema.keys {
        if key.secret || key.default_value.is_some() || key.reserved {
            continue;
        }
        let shape = key
            .constraint
            .as_ref()
            .and_then(|constraint| constraint.get("type"))
            .and_then(Json::as_str);
        let name = key.path.rsplit('.').next().unwrap_or(&key.path);
        // Only bare, unquoted names can be found by a substring search without reimplementing the
        // writer's quoting; the rest are covered by the corpus.
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            || name.is_empty()
        {
            continue;
        }
        let Some(line) = rendered.lines().find(|line| {
            line.trim_start_matches("# ")
                .starts_with(&format!("{name} = "))
        }) else {
            continue;
        };
        let value = line.rsplit(" = ").next().unwrap_or_default();

        match shape {
            Some("integer") => assert_eq!(value, "0", "an integer placeholder is a bare `0`"),
            Some("number") => assert_eq!(value, "0.0", "a float placeholder keeps its point"),
            Some("boolean") => assert_eq!(value, "false", "a boolean placeholder is `false`"),
            Some("array") => assert_eq!(value, "[]", "an array placeholder is empty"),
            _ => assert!(
                value.starts_with('"'),
                "a placeholder for an unrecognised shape is a quoted string, and `{value}` is not"
            ),
        }
    }
}
