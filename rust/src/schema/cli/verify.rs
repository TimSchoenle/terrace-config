//! Reading back what a build actually produced.
//!
//! [`Contract::check_labels`](crate::schema::Contract::check_labels) does the comparison and knows
//! nothing about where either side came from. This module is the other half: turning what
//! `docker inspect` prints and what a Dockerfile contains into the values that comparison takes.
//!
//! It is separate from the rest of the module because the two are used at different moments by
//! different callers. The renderer runs inside the build, from the source tree. These run *after*
//! the build, against an image — in a test, a small binary, or a CI step that has an image and no
//! Rust toolchain opinion beyond being able to run one.
//!
//! # Two checks, and neither replaces the other
//!
//! [`dockerfile_block`] is the cheap half: regenerate the block and diff it against the committed
//! Dockerfile. It runs in the pull request that renamed a key, in a diff a reviewer reads, and it
//! needs no image.
//!
//! [`labels_from_json`] feeds the half that costs an image and catches what no source diff can: a
//! build argument that failed to interpolate, a label a base image overrode, a `LABEL` line
//! deleted on a branch nobody diffed.

use std::collections::BTreeMap;

use crate::schema::{Error, MARKER_BEGIN, MARKER_END};

/// The labels of a built image, from whatever printed them.
///
/// Accepts three shapes, because three tools spell the same thing differently and reading the
/// wrong one is the classic way to make this check pass without comparing anything:
///
/// - the labels object itself — `docker inspect --format '{{json .Config.Labels}}'`;
/// - a `docker inspect` config object, under `Config.Labels`;
/// - a `crane config` object, under `config.Labels`.
///
/// # Errors
///
/// Returns [`Error::Invalid`] rather than an empty map for the two inputs that otherwise look like
/// success, because a comparison against nothing passes:
///
/// - `null`, which is what reading the wrong JSON path yields. An image with no labels at all
///   reports `{}`; a `null` means the path was wrong, and that is a broken check rather than a
///   passing image.
/// - a value that is not an object at all, or an object whose values are not strings.
///
/// An empty object is *accepted* here and fails in the comparison instead, naming the labels that
/// are missing — which is the more useful message, and the correct place for the judgement.
pub fn labels_from_json(json: &str) -> Result<BTreeMap<String, String>, Error> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| Error::Invalid(format!("the labels are not JSON: {e}")))?;

    // Unwrapped in this order so that a full `docker inspect` element, whose `Config` object also
    // contains `Labels`, is reached before the top level is judged.
    let labels = value
        .get("Config")
        .and_then(|config| config.get("Labels"))
        .or_else(|| value.get("config").and_then(|config| config.get("Labels")))
        .unwrap_or(&value);

    match labels {
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(name, value)| match value.as_str() {
                Some(text) => Ok((name.clone(), text.to_owned())),
                None => Err(Error::Invalid(format!(
                    "the image's `{name}` label is {}, and a label is a string. This is not an \
                     image's label set.",
                    kind(value)
                ))),
            })
            .collect(),
        serde_json::Value::Null => Err(Error::Invalid(
            "the labels are `null`, which is what reading the wrong JSON path yields — an image \
             with no labels at all reports `{}`. `docker inspect` says `.Config.Labels` and \
             `crane config` says `.config.Labels`."
                .to_owned(),
        )),
        other => Err(Error::Invalid(format!(
            "the labels are {}, not an object, so nothing was compared.",
            kind(other)
        ))),
    }
}

/// The generated label region of a Dockerfile, between its markers, with no trailing newline.
///
/// Cut at [`MARKER_BEGIN`] and [`MARKER_END`] rather than by line count, so a fourth label added
/// upstream is compared like the first three instead of silently falling outside the window. Diff
/// the result against [`Contract::to_dockerfile_labels`](crate::schema::Contract::to_dockerfile_labels)
/// — the block without its markers — or compare the whole committed region against
/// [`Contract::to_dockerfile_block`](crate::schema::Contract::to_dockerfile_block).
///
/// # Errors
///
/// Returns [`Error::Invalid`] if the region is absent, unterminated, out of order, or empty. An
/// empty region is refused for the same reason a `null` label set is: it would compare equal to
/// nothing and report success, which is the one failure this whole scheme cannot afford.
pub fn dockerfile_block(dockerfile: &str) -> Result<&str, Error> {
    let begin = dockerfile.find(MARKER_BEGIN).ok_or_else(|| {
        Error::Invalid(format!(
            "the Dockerfile carries no `{MARKER_BEGIN}` line, so the generated label block has \
             nowhere to go and nothing to be compared against. `Contract::to_dockerfile_block` \
             emits the region, markers included."
        ))
    })?;
    let after_begin = begin + MARKER_BEGIN.len();

    let end = dockerfile[after_begin..]
        .find(MARKER_END)
        .map(|offset| after_begin + offset)
        .ok_or_else(|| {
            Error::Invalid(format!(
                "the Dockerfile opens a `{MARKER_BEGIN}` region and never closes it with \
                 `{MARKER_END}`."
            ))
        })?;

    let block = dockerfile[after_begin..end].trim_matches(['\n', '\r']);
    if block.is_empty() {
        return Err(Error::Invalid(format!(
            "the `{MARKER_BEGIN}` region is empty, so a comparison against it would pass without \
             checking a single label."
        )));
    }
    Ok(block)
}

/// What a JSON value is, for a message about having read the wrong one.
fn kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "`null`",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}
