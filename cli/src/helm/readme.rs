//! The credential reference each chart's README carries, written from the contract.
//!
//! A chart that mounts credentials as files has to tell an operator three things: which credentials
//! the image needs, what to name the keys of the Secret that carries them, and which environment
//! spelling addresses the same value. All three are published by the image — `secrets_file`, `env`
//! and `env_file` per key, with `required` and the producer's own summary beside them — and all
//! three used to be transcribed into every README by hand.
//!
//! The transcription drifts, and it drifts silently, because nothing downstream reads a README. The
//! measured case listed seven credentials against the sixteen its nine contracts declare, spelt one
//! of them under a name no contract carries, and marked another required that only one service
//! requires. Every one of those was true when it was written.
//!
//! So the rows come from the contract. What stays hand-written is the one column no contract can
//! fill: *when this chart needs the credential*. That is chart knowledge — a service being enabled,
//! a storage backend being chosen, "first install only" — and it lives in the declaration beside the
//! keys it describes, where a note for a credential the image stopped declaring is refused rather
//! than left standing.
//!
//! # Where the block goes
//!
//! Between two HTML comments in the chart's `README.md.gotmpl`:
//!
//! ```text
//! <!-- @config-credentials -->
//! ...generated...
//! <!-- @config-credentials end -->
//! ```
//!
//! In the template rather than in the rendered `README.md`, because whatever regenerates the second
//! from the first would overwrite anything written there. HTML comments rather than a marker of this
//! repository's own, because they survive the round trip through a documentation generator and
//! through every Markdown renderer that publishes the result, and a reader of the published page
//! sees nothing.
//!
//! The chart chooses *where* the block sits; this module owns only what is between the markers. A
//! chart whose contracts declare a credential and which carries no block is a failure rather than a
//! skip — the whole point is that the reference cannot be forgotten — and a block in a chart with no
//! credential is a failure for the mirror reason.
//!
//! # What is deliberately not generated
//!
//! **The `kubectl create secret` example.** It carries a Secret name, a namespace placeholder and,
//! for several charts, a key-length decision that is the chart's own. None of that is in a contract,
//! and a generated example is one nobody has checked against the chart's own prose two paragraphs
//! above.
//!
//! **Everything a credential means.** The producer's summary is one line and it is rendered; the
//! paragraph explaining what losing a pepper costs is the chart's, and it stays in the chart's prose.

use std::path::Path;

use serde_json::Value as Json;

use crate::error::Error;
use crate::union::union_contracts;

use super::bindings::has_path;
use super::declaration::{Declaration, Document, vendored_for};
use super::secrets::{Credential, contracted_charts, credentials, declared_secrets};

/// The two comments the generated block sits between.
///
/// Read as whole lines with their indentation ignored, so a chart may sit them inside a list item
/// without the reader caring.
pub const OPEN: &str = "<!-- @config-credentials -->";
/// The closing marker.
pub const CLOSE: &str = "<!-- @config-credentials end -->";

/// The file the block lives in.
pub const TEMPLATE: &str = "README.md.gotmpl";

/// What one pass over the chart tree found.
#[derive(Debug, Default)]
pub struct Written {
    /// Everything that stopped a reference being written or made it wrong.
    pub problems: Vec<String>,
    /// How many templates were rewritten.
    pub touched: usize,
}

/// Write, or compare, every contracted chart's credential reference.
///
/// # Errors
/// [`Error::Invalid`] when a declaration or a contract cannot be read at all, [`Error::Io`] when the
/// tree cannot be walked or a template cannot be written. A chart whose *reference* is wrong is a
/// problem on the result rather than an error: the run still answers for every other chart.
pub fn walk(charts: &Path, check: bool) -> Result<Written, Error> {
    let mut written = Written::default();
    for (chart_dir, declaration) in contracted_charts(charts)? {
        one_chart(&chart_dir, &declaration, check, &mut written)?;
    }
    Ok(written)
}

/// One chart's template.
fn one_chart(
    chart_dir: &Path,
    declaration: &Declaration,
    check: bool,
    written: &mut Written,
) -> Result<(), Error> {
    let template = chart_dir.join(TEMPLATE);
    let rows = credentials(&declared_secrets(chart_dir, declaration)?);

    if !template.is_file() {
        if !rows.is_empty() {
            written.problems.push(format!(
                "{}: declares {} credential(s) and has no {TEMPLATE} to write the reference into",
                declaration.chart,
                rows.len()
            ));
        }
        return Ok(());
    }

    let text = std::fs::read_to_string(&template).map_err(|e| Error::io(template.display(), e))?;

    if !carries_markers(&text) {
        if !rows.is_empty() {
            written.problems.push(format!(
                "{}: this chart's contracts declare {} credential(s) and the template carries no \
                 `{OPEN}` block for them. Add the two markers where the reference belongs and \
                 regenerate",
                template.display(),
                rows.len()
            ));
        }
        return Ok(());
    }

    if rows.is_empty() {
        written.problems.push(format!(
            "{}: carries a `{OPEN}` block and no contract this chart declares marks any key \
             `secret: true`, so there is nothing to generate into it",
            template.display()
        ));
        return Ok(());
    }

    let values = super::read_file(&chart_dir.join("values.yaml"))?;
    let block = match block_for(chart_dir, declaration, &rows, &values) {
        Ok(block) => block,
        Err(problem) => {
            written.problems.push(problem);
            return Ok(());
        }
    };
    let updated = match splice(&text, &block, &template.display().to_string()) {
        Ok(updated) => updated,
        Err(problem) => {
            written.problems.push(problem);
            return Ok(());
        }
    };

    if updated == text {
        return Ok(());
    }
    if check {
        written.problems.push(format!(
            "{}: the credential reference is not what the contract describes; regenerate it\n{}",
            template.display(),
            difference(&text, &updated, &template.display().to_string())
        ));
        return Ok(());
    }

    std::fs::write(&template, updated).map_err(|e| Error::io(template.display(), e))?;
    written.touched += 1;
    Ok(())
}

/// The generated credential reference for one chart, as the lines between the markers.
///
/// # Errors
/// The message describing a declaration that has outlived what it described. An [`Err`] here is a
/// problem with one chart rather than with the run, which is why it is a `String` and not an
/// [`Error`].
pub fn block_for(
    chart_dir: &Path,
    declaration: &Declaration,
    rows: &[Credential],
    values: &Json,
) -> Result<Vec<String>, String> {
    let notes = &declaration.credentials;
    still_describes_something(declaration, rows, values)?;

    // Each optional column appears only where it carries something. `Read by` on a single-document
    // chart would be that chart's own name repeated once per row, and an empty column is one more
    // thing between an operator and the key name they came for.
    let multi = declaration.documents.len() > 1;
    let has_chart_values = notes.values().any(|entry| entry.value.is_some());
    let annotated = notes.values().any(|entry| entry.note.is_some());

    let mut columns = vec!["Secrets file", "Required"];
    if has_chart_values {
        columns.push("Chart value");
    }
    if multi {
        columns.push("Read by");
    }
    if annotated {
        columns.push("When");
    }

    let mut lines = vec![
        "Every credential below is read from a **file in the secrets directory**, named for the"
            .to_owned(),
        "configuration path it carries with `.` written as `__`. A file spelt any other way is"
            .to_owned(),
        "mounted and never read. Those names are the keys of the Secret this chart mounts, except"
            .to_owned(),
        "where a note below says otherwise.".to_owned(),
        String::new(),
        format!("| {} |", columns.join(" | ")),
        format!("|{}|", vec!["---"; columns.len()].join("|")),
    ];

    for row in rows {
        let entry = notes.get(&row.path);
        let mut cells = vec![
            format!("`{}`", row.secrets_file),
            if row.required { "yes" } else { "no" }.to_owned(),
        ];
        if has_chart_values {
            cells.push(
                entry
                    .and_then(|entry| entry.value.as_deref())
                    .map_or_else(String::new, |value| format!("`{value}`")),
            );
        }
        if multi {
            cells.push(
                row.documents
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        if annotated {
            cells.push(
                entry
                    .and_then(|entry| entry.note.clone())
                    .unwrap_or_default(),
            );
        }
        lines.push(format!("| {} |", cells.join(" | ")));
    }

    if let Some(prefix) = env_prefix(chart_dir, declaration) {
        lines.extend([
            String::new(),
            format!("The same value is addressable as the variable `{prefix}<PATH>`, upper-cased with `.`"),
            "written as `__`, and that spelling with `_FILE` appended names a file whose contents"
                .to_owned(),
            "supply it.".to_owned(),
        ]);
    }

    Ok(lines)
}

/// Refuse a hand-written note that has outlived what it described.
///
/// Both halves are the same failure seen from two sides: a reference telling an operator about a
/// credential the image stopped declaring, or telling them to set a chart value that no longer
/// exists. Either one is prose that was true when it was written, which is the whole reason none of
/// the rest of this block is hand-written.
fn still_describes_something(
    declaration: &Declaration,
    rows: &[Credential],
    values: &Json,
) -> Result<(), String> {
    let notes = &declaration.credentials;

    let unknown: Vec<&String> = notes
        .keys()
        .filter(|key| !rows.iter().any(|row| &&row.path == key))
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "{}: `credentials` carries an entry for {}, which no contract this chart declares \
             marks `secret: true` — the entry has outlived the credential it described",
            declaration.path.display(),
            unknown
                .iter()
                .map(|key| crate::gate::quoted(key))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let mut missing: Vec<&str> = notes
        .values()
        .filter_map(|entry| entry.value.as_deref())
        .filter(|value| !has_path(values, value))
        .collect();
    missing.sort_unstable();
    if !missing.is_empty() {
        return Err(format!(
            "{}: `credentials` names the chart value(s) {}, which this chart's values.yaml does \
             not expose — a renamed value leaves the reference telling operators to set something \
             that does nothing",
            declaration.path.display(),
            missing
                .iter()
                .map(|name| crate::gate::quoted(name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(())
}

/// The variable prefix every document of one chart agrees on, or [`None`] when they do not.
///
/// Taken from the contract's own dialect rather than guessed from the longest common prefix of the
/// spellings. The guess reads as the whole variable, presented as the prefix, on a chart with
/// exactly one credential — which is precisely the chart whose operator has the least context for
/// spotting that the sentence is nonsense.
#[must_use]
pub fn env_prefix(chart_dir: &Path, declaration: &Declaration) -> Option<String> {
    let mut held: Option<String> = None;
    for document in &declaration.documents {
        let prefix = document_prefix(chart_dir, document)?;
        match &held {
            None => held = Some(prefix),
            Some(seen) if seen == &prefix => {}
            Some(_) => return None,
        }
    }
    held.filter(|prefix| !prefix.is_empty())
}

/// One document's dialect prefix, or [`None`] when its contracts cannot be read or merged.
fn document_prefix(chart_dir: &Path, document: &Document) -> Option<String> {
    let loaded = vendored_for(chart_dir, document).ok()?;
    let contracts: Vec<(String, Json)> = loaded
        .iter()
        .map(|item| (item.label.clone(), item.vendored.contract.clone()))
        .collect();
    let union = union_contracts(&contracts).ok()?;
    Some(union.prefix().to_owned())
}

/// One README template with the block between its markers replaced.
///
/// The markers are matched as whole lines and their own indentation is preserved, so the block a
/// chart nests inside a list item stays nested. Anything but exactly one pair is refused: a second
/// pair would make "the block" ambiguous, and an unclosed one would swallow the rest of the file.
///
/// # Errors
/// The message describing markers this cannot splice between.
pub fn splice(text: &str, block: &[String], where_: &str) -> Result<String, String> {
    let ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<&str> = text.split(ending).collect();

    let found = |marker: &str| -> Vec<usize> {
        lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.trim() == marker)
            .map(|(number, _)| number)
            .collect()
    };
    let (opens, closes) = (found(OPEN), found(CLOSE));
    if opens.len() != 1 || closes.len() != 1 {
        return Err(format!(
            "{where_}: carries {} `{OPEN}` and {} `{CLOSE}` markers, and a generated block is \
             delimited by exactly one of each",
            opens.len(),
            closes.len()
        ));
    }
    if closes[0] < opens[0] {
        return Err(format!(
            "{where_}: the `{CLOSE}` marker sits above the `{OPEN}` one"
        ));
    }

    let indent: String = lines[opens[0]]
        .chars()
        .take_while(|held| held.is_whitespace())
        .collect();
    let mut written: Vec<String> = lines[..=opens[0]]
        .iter()
        .map(|&line| line.to_owned())
        .collect();
    written.extend(block.iter().map(|line| {
        if line.is_empty() {
            String::new()
        } else {
            format!("{indent}{line}")
        }
    }));
    written.extend(lines[closes[0]..].iter().map(|&line| line.to_owned()));
    Ok(written.join(ending))
}

/// Whether a template carries the opening marker at all.
#[must_use]
pub fn carries_markers(text: &str) -> bool {
    text.lines().any(|line| line.trim() == OPEN)
}

/// The lines that moved, as a reviewer of a failing check wants to see them.
///
/// A whole-file unified diff would print a README, and the only lines a reader needs are the ones
/// the contract disagrees with. Deliberately not a minimal edit script: this compares two renderings
/// of one generated block, so an aligned comparison would spend a dependency on making an already
/// short answer marginally shorter.
fn difference(before: &str, after: &str, at: &str) -> String {
    use std::fmt::Write as _;

    let (before, after): (Vec<&str>, Vec<&str>) =
        (before.lines().collect(), after.lines().collect());
    let mut out = format!("--- {at}\n+++ the contract\n");
    for line in before.iter().filter(|line| !after.contains(line)) {
        let _ = writeln!(out, "-{line}");
    }
    for line in after.iter().filter(|line| !before.contains(line)) {
        let _ = writeln!(out, "+{line}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{CLOSE, OPEN, carries_markers, splice};

    fn block() -> Vec<String> {
        vec!["one".to_owned(), String::new(), "two".to_owned()]
    }

    #[test]
    fn the_block_replaces_what_is_between_the_markers_and_nothing_else() {
        let text = format!("head\n{OPEN}\nold\n{CLOSE}\ntail\n");
        assert_eq!(
            splice(&text, &block(), "x").expect("one pair of markers"),
            format!("head\n{OPEN}\none\n\ntwo\n{CLOSE}\ntail\n")
        );
    }

    #[test]
    fn a_nested_block_stays_nested_and_a_blank_line_gains_no_trailing_space() {
        // A chart may sit the reference inside a list item. Indenting the blank line too would put
        // trailing whitespace in a generated file, which every linter in a chart repository refuses.
        let text = format!("- head\n  {OPEN}\n  {CLOSE}\n");
        assert_eq!(
            splice(&text, &block(), "x").expect("one pair of markers"),
            format!("- head\n  {OPEN}\n  one\n\n  two\n  {CLOSE}\n")
        );
    }

    #[test]
    fn the_line_ending_the_file_already_uses_is_the_one_written_back() {
        let text = format!("head\r\n{OPEN}\r\nold\r\n{CLOSE}\r\n");
        let written = splice(&text, &block(), "x").expect("one pair of markers");
        assert!(!written.contains("\n\n"), "{written:?}");
        assert!(written.contains("\r\none\r\n"), "{written:?}");
    }

    #[test]
    fn anything_but_one_pair_is_refused_rather_than_guessed_at() {
        // A second pair makes "the block" ambiguous, and an unclosed one would swallow the file.
        for text in [
            format!("{OPEN}\n{CLOSE}\n{OPEN}\n{CLOSE}\n"),
            format!("{OPEN}\n"),
            format!("{CLOSE}\n{OPEN}\n"),
        ] {
            assert!(splice(&text, &block(), "x").is_err(), "{text:?}");
        }
    }

    #[test]
    fn a_marker_is_a_whole_line_and_not_a_mention_of_one() {
        // A README that documents the marker in prose still has one block, not two.
        assert!(!carries_markers(&format!(
            "write `{OPEN}` where it belongs\n"
        )));
        assert!(carries_markers(&format!("  {OPEN}  \n")));
    }
}
