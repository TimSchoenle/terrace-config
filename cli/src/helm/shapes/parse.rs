//! Reading the `@config-shape` markers out of values.yaml, and the block each one heads.

use serde_json::{Map, Value as Json};

use crate::error::Error;

use crate::helm::markers::{Block, MARKER};

use super::{
    Departure, Divergence, EXCEPT_MARKER, GENERATED, HANDWRITTEN, NARROW_MARKER, SHAPE_MARKER,
    Shape,
};

/// Every shape marker in one values file.
///
/// Every problem is collected before any is raised, which is the posture every rule here takes: one
/// broken line must not hide the state of the rest.
///
/// # Errors
/// [`Error::Invalid`] listing every marker that cannot be read as one.
pub fn parse_markers(text: &str, chart: &str) -> Result<(Vec<Shape>, Vec<Divergence>), Error> {
    let mut shapes = Vec::new();
    let mut divergences = Vec::new();
    let mut problems: Vec<String> = Vec::new();

    for (offset, line) in text.lines().enumerate() {
        let number = offset + 1;
        let at = format!("{chart}/values.yaml:{number}");

        // The suffixed spellings are matched first, since both their names begin with the bare one.
        if let Some(rest) = marker_body(line, EXCEPT_MARKER) {
            read_divergence(
                &mut divergences,
                &mut problems,
                chart,
                number,
                &at,
                rest,
                Departure::Override,
            );
            continue;
        }
        if let Some(rest) = marker_body(line, NARROW_MARKER) {
            read_divergence(
                &mut divergences,
                &mut problems,
                chart,
                number,
                &at,
                rest,
                Departure::Narrowing,
            );
            continue;
        }
        let Some(rest) = marker_body(line, SHAPE_MARKER) else {
            continue;
        };

        let words: Vec<&str> = rest.split_whitespace().collect();
        if words.len() >= 2 && words[1] == GENERATED {
            problems.push(format!(
                "{at}: `{SHAPE_MARKER} {} {GENERATED}` is obsolete — every value carrying a \
                 `{MARKER}` marker has its `@schema` block generated from the contract now, \
                 without enrolling. Delete this line; declare any position the chart keeps with \
                 `{EXCEPT_MARKER}` or `{NARROW_MARKER}`",
                words[0]
            ));
            continue;
        }
        if words.len() < 2 || words[1] != HANDWRITTEN {
            problems.push(format!(
                "{at}: `{SHAPE_MARKER}` takes a values path and then `{HANDWRITTEN} <release> \
                 <source>`, which is the one thing a marker still declares"
            ));
            continue;
        }
        if words.len() != 4 {
            problems.push(format!(
                "{at}: `{SHAPE_MARKER} {} {HANDWRITTEN}` takes the release the struct was read at \
                 and the file it was read from",
                words[0]
            ));
            continue;
        }

        shapes.push(Shape {
            chart: chart.to_owned(),
            line: number,
            values_path: words[0].to_owned(),
            handwritten: true,
            version: Some(words[2].to_owned()),
            source: Some(words[3].to_owned()),
            declared: true,
        });
    }

    if problems.is_empty() {
        Ok((shapes, divergences))
    } else {
        Err(Error::Invalid(problems.join("\n")))
    }
}

/// One departure marker's three words.
pub(super) fn read_divergence(
    divergences: &mut Vec<Divergence>,
    problems: &mut Vec<String>,
    chart: &str,
    line: usize,
    at: &str,
    rest: &str,
    kind: Departure,
) {
    let mut words = rest.splitn(3, char::is_whitespace);
    let (Some(values_path), Some(sub_path), Some(reason)) =
        (words.next(), words.next(), words.next())
    else {
        problems.push(format!(
            "{at}: `{}` takes a values path, a sub-path inside the schema and the reason the \
             chart keeps its own — a departure from the contract without a reason is one nobody \
             can review",
            kind.marker()
        ));
        return;
    };
    let reason = reason.trim();
    if sub_path.is_empty() || reason.is_empty() {
        problems.push(format!(
            "{at}: `{}` takes a values path, a sub-path inside the schema and the reason the \
             chart keeps its own — a departure from the contract without a reason is one nobody \
             can review",
            kind.marker()
        ));
        return;
    }

    divergences.push(Divergence {
        chart: chart.to_owned(),
        line,
        values_path: values_path.to_owned(),
        sub_path: sub_path.to_owned(),
        reason: reason.to_owned(),
        kind,
    });
}

/// The body of one whole-line comment marker, if the line is one.
///
/// Anchored as a whole comment line, so a marker mentioned in prose — a chart's own `# --`
/// description, this file's documentation — is not mistaken for a declaration.
pub(super) fn marker_body<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let rest = line.trim_start().strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix(marker)?;
    let body = rest.strip_prefix(|held: char| held.is_whitespace())?.trim();
    (!body.is_empty()).then_some(body)
}

/// One block's lines, as `(the marker run, the schema)`.
///
/// The marker run is the `# # @config ...` lines the block opens with — hand-written, and none of
/// this module's business. Everything after it is the schema, and is what a regeneration replaces.
pub fn split_block(block: &Block) -> (Vec<String>, Vec<String>) {
    let mut markers = Vec::new();
    let mut rest = Vec::new();
    let mut in_run = true;
    for line in &block.lines {
        if in_run && crate::helm::markers::opens_with_a_marker(line) {
            markers.push(line.clone());
            continue;
        }
        in_run = false;
        rest.push(line.clone());
    }
    (markers, rest)
}

/// The schema half of one block, as the mapping a schema generator reads it as.
///
/// Compared rather than the text, so a difference in quoting or in key order is not reported as
/// drift: what the block *means* is what the contract has an opinion about.
///
/// # Errors
/// [`Error::Invalid`] when a line inside the block is not a comment, or the body is not a mapping.
pub fn block_schema(block: &Block) -> Result<Json, Error> {
    let (_, lines) = split_block(block);
    let mut body = Vec::new();
    for line in &lines {
        let stripped = line.trim();
        let Some(rest) = stripped.strip_prefix('#') else {
            return Err(Error::Invalid(format!(
                "line '{line}' inside an `@schema` block is not a comment"
            )));
        };
        // The single space a schema generator's `# ` prefix carries, and nothing more: the
        // schema's own indentation is what makes it a mapping.
        body.push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
    }
    if body.iter().all(|line| line.trim().is_empty()) {
        return Ok(Json::Object(Map::new()));
    }
    let loaded: Json = serde_norway::from_str(&body.join("\n"))
        .map_err(|failure| Error::Invalid(format!("an `@schema` block is not YAML: {failure}")))?;
    match loaded {
        Json::Null => Ok(Json::Object(Map::new())),
        value if value.is_object() => Ok(value),
        _ => Err(Error::Invalid(
            "an `@schema` block has to be a mapping".to_owned(),
        )),
    }
}
