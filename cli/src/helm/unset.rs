//! What a bound chart value left at `null` does, written beside the value.
//!
//! An optional key with no published default is written as `null`, and its projection wraps it in
//! a guard, so the key never reaches the configuration document and the image applies its own
//! default. That is correct and it is invisible: an operator reading `accessTtlMinutes: null` in
//! `values.yaml`, or `null` in the README's Default column, learns neither that `null` means "the
//! image decides" nor what the image decides.
//!
//! The line that says so is helm-docs' own default override, written as the last line of the
//! value's description:
//!
//! ```text
//! # -- How long an access token stays valid (`auth.access_ttl_minutes`).
//! # @default -- `null` (unset): the key is left out and the image default applies, `15`.
//! accessTtlMinutes: null
//! ```
//!
//! Chosen over a sentence inside the description for three reasons, each measured against the
//! pinned generators. helm-docs renders it as the README's Default column in place of the bare
//! `null`; helm-schema strips every helm-docs `@` tag from the description it copies into
//! `values.schema.json`, so the editor hover is unchanged; and it is one line with a fixed prefix,
//! so the writer that owns it can find it again without parsing prose it did not write.
//!
//! # The text comes from the contract, and nothing else
//!
//! The default named is the contract's `default`, the rendering the producer publishes for display,
//! with `note` qualifying it. A contract that publishes none gets a line that says so rather than a
//! guess: the image still has a compiled default, and the fix for not knowing it is the producer
//! publishing it, which the next refresh then writes here without anyone editing the chart.
//!
//! A key several documents declare is one value, and the documents can disagree about its default.
//! The line then names each document's, so the disagreement is visible where it takes effect.

use std::fmt::Write as _;

use serde_json::{Map, Value as Json};

/// helm-docs' default override, exactly as its parser matches it: `^\s*# @default -- (.*)$`.
pub const ANNOTATION: &str = "# @default -- ";

/// How every sentence opens: the literal value, then what the projection's guard does with it.
const LEFT_OUT: &str = "`null` (unset): the key is left out and";

/// What one document publishes about a key's default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    /// The document, when the key is resolved against several; [`None`] for one.
    pub document: Option<String>,
    /// The default as the producer renders it for display, or [`None`] when it publishes none.
    pub default: Option<String>,
    /// Prose qualifying the default, e.g. `permanent` for a zero that means no expiry.
    pub note: Option<String>,
}

impl Published {
    /// Reads one contract entry's `default`, `default_value` and `note`.
    ///
    /// `default` is preferred because it is the producer's own rendering for a reader. A document
    /// that carries only `default_value` still has a default, so that is rendered as JSON rather
    /// than reported as missing.
    pub fn of(document: Option<String>, fields: &Map<String, Json>) -> Self {
        let default = match fields.get("default") {
            Some(Json::String(text)) => Some(text.clone()),
            Some(Json::Null) | None => fields
                .get("default_value")
                .filter(|held| !held.is_null())
                .map(Json::to_string),
            Some(other) => Some(other.to_string()),
        };
        let note = fields
            .get("note")
            .and_then(Json::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned);
        Self {
            document,
            default,
            note,
        }
    }

    /// The default and its note, as one phrase: `` `0` (permanent) ``, or [`None`] with no default.
    fn phrase(&self) -> Option<String> {
        let mut phrase = code(self.default.as_deref()?);
        if let Some(note) = &self.note {
            let _ = write!(phrase, " ({})", prose(note));
        }
        Some(phrase)
    }
}

/// What `null` does for a value fed by a key these documents publish, as one sentence.
///
/// It never claims a default the contract does not name: a key with none may be an `Option` that
/// unset turns off, or a default the producer did not publish, and the contract cannot tell the
/// two apart. Three forms, by how many distinct answers the documents give, each opening with
/// `` `null` (unset): the key is left out and ``:
///
/// - one, with a default: `` the image default applies, `15`. ``
/// - one, without: `the image decides; its contract publishes no default.`
/// - several: `` each image decides: api, sync default `15`; worker no published default. ``
pub fn sentence(published: &[Published]) -> String {
    let mut groups: Vec<(Option<String>, Vec<&str>)> = Vec::new();
    for one in published {
        let phrase = one.phrase();
        let document = one.document.as_deref().unwrap_or_default();
        match groups.iter_mut().find(|(held, _)| *held == phrase) {
            Some((_, documents)) => documents.push(document),
            None => groups.push((phrase, vec![document])),
        }
    }

    match groups.as_slice() {
        [] | [(None, _)] => {
            format!("{LEFT_OUT} the image decides; its contract publishes no default.")
        }
        [(Some(phrase), _)] => format!("{LEFT_OUT} the image default applies, {phrase}."),
        _ => {
            for (_, documents) in &mut groups {
                documents.sort_unstable();
                documents.dedup();
            }
            groups.sort_by(|left, right| left.1.cmp(&right.1));
            let each = groups
                .iter()
                .map(|(phrase, documents)| {
                    let documents = documents.join(", ");
                    match phrase {
                        Some(phrase) => format!("{documents} default {phrase}"),
                        None => format!("{documents} no published default"),
                    }
                })
                .collect::<Vec<_>>()
                .join("; ");
            format!("{LEFT_OUT} each image decides: {each}.")
        }
    }
}

/// The annotation line for `sentence`, at `indent`.
pub fn line(sentence: &str, indent: &str) -> String {
    format!("{indent}{ANNOTATION}{sentence}")
}

/// Whether a values-file line is a default override, whoever wrote it.
pub fn is_annotation(line: &str) -> bool {
    line.trim_start().starts_with(ANNOTATION)
}

/// One default as a Markdown code span that survives a README table cell.
///
/// Whitespace collapses to single spaces, because helm-docs reads one line; `|` is escaped, because
/// an unescaped one ends the table cell even inside a code span; and the fence is one backtick
/// longer than the longest run inside, so a default holding a backtick still renders as code. An
/// empty default is written `""`, since an empty code span renders as nothing at all.
fn code(text: &str) -> String {
    let collapsed = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|");
    if collapsed.is_empty() {
        return "`\"\"`".to_owned();
    }
    let mut longest = 0;
    let mut run = 0;
    for character in collapsed.chars() {
        if character == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat(longest + 1);
    if longest == 0 {
        format!("{fence}{collapsed}{fence}")
    } else {
        format!("{fence} {collapsed} {fence}")
    }
}

/// Prose for a table cell: one line, with `|` escaped.
fn prose(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Published, is_annotation, line, sentence};

    fn published(document: Option<&str>, fields: &serde_json::Value) -> Published {
        Published::of(
            document.map(str::to_owned),
            fields.as_object().expect("an object"),
        )
    }

    #[test]
    fn a_published_default_is_named() {
        let one = published(None, &json!({"default": "15", "default_value": 15}));
        assert_eq!(
            sentence(&[one]),
            "`null` (unset): the key is left out and the image default applies, `15`."
        );
    }

    #[test]
    fn a_note_qualifies_the_default() {
        let one = published(None, &json!({"default": "0", "note": "permanent"}));
        assert_eq!(
            sentence(&[one]),
            "`null` (unset): the key is left out and the image default applies, `0` (permanent)."
        );
    }

    #[test]
    fn a_missing_default_is_said_rather_than_guessed() {
        let one = published(None, &json!({"default": null, "default_value": null}));
        assert_eq!(
            sentence(&[one]),
            "`null` (unset): the key is left out and the image decides; its contract publishes no default."
        );
    }

    #[test]
    fn a_default_value_without_a_rendering_is_still_a_default() {
        let one = published(
            None,
            &json!({"default": null, "default_value": "production"}),
        );
        assert_eq!(
            sentence(&[one]),
            "`null` (unset): the key is left out and the image default applies, `\"production\"`."
        );
    }

    #[test]
    fn documents_that_agree_read_as_one() {
        let fields = json!({"default": "30"});
        let both = [
            published(Some("worker"), &fields),
            published(Some("api"), &fields),
        ];
        assert_eq!(
            sentence(&both),
            "`null` (unset): the key is left out and the image default applies, `30`."
        );
    }

    #[test]
    fn documents_that_disagree_are_each_named() {
        let all = [
            published(Some("worker"), &json!({"default": null})),
            published(Some("sync"), &json!({"default": "15"})),
            published(Some("api"), &json!({"default": "15"})),
        ];
        assert_eq!(
            sentence(&all),
            "`null` (unset): the key is left out and each image decides: api, sync default `15`; worker no published default."
        );
    }

    #[test]
    fn a_default_that_would_break_the_table_or_the_span_is_escaped() {
        let one = published(None, &json!({"default": "a|b `c`\nd"}));
        assert_eq!(
            sentence(&[one]),
            "`null` (unset): the key is left out and the image default applies, `` a\\|b `c` d ``."
        );
        let empty = published(None, &json!({"default": ""}));
        assert_eq!(
            sentence(&[empty]),
            "`null` (unset): the key is left out and the image default applies, `\"\"`."
        );
    }

    #[test]
    fn the_line_is_the_one_helm_docs_matches() {
        let written = line("x", "  ");
        assert_eq!(written, "  # @default -- x");
        assert!(is_annotation(&written));
        assert!(!is_annotation("  # -- @default -- x"));
        assert!(!is_annotation("  # @defaults -- x"));
    }
}
