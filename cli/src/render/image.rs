//! How the contract is found on the image: the labels, and the block that carries them.
//!
//! Both renderings are a pure function of the document plus one argument, the in-image path, which
//! is the build's choice and not the document's. That is why they are here rather than in a
//! producer: a Java build and a Rust build write the same three labels, and the day they do not is
//! the day a chart's discovery step silently finds nothing.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::Error;
use crate::document::{Contract, MARKER_BEGIN, MARKER_END};

/// The labels as `NAME=value` lines, one per label, no trailing newline.
///
/// The shape a `--label` loop or a `docker build --label "$(…)"` wants.
pub fn labels(contract: &Contract, path: &str) -> String {
    contract
        .labels(path)
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The labels as a `LABEL` instruction, ready to paste into a Dockerfile.
///
/// A `LABEL` key cannot be interpolated from anything, so the choice is between a Dockerfile that
/// spells these by hand and a build that passes `--label` from a host-side run of a generator.
/// Multi-stage builds make the second awkward — the document is produced *inside* a builder stage,
/// where the host's `docker build` command line cannot reach it — so the honest answer is to make
/// hand-writing a copy-paste and then check the result, which is what [`check_labels`] is for.
///
/// Ends with a newline. No trailing backslash, so a following instruction needs no separator.
pub fn dockerfile_labels(contract: &Contract, path: &str) -> String {
    let labels = contract.labels(path);
    let mut rendered = String::from("LABEL ");
    for (index, (name, value)) in labels.iter().enumerate() {
        if index > 0 {
            rendered.push_str(" \\\n      ");
        }
        rendered.push_str(name);
        rendered.push_str("=\"");
        // A value carrying either of these would break the instruction rather than the build,
        // which is the worst way to find out. Neither occurs in a prefix or a path today.
        for character in value.chars() {
            if character == '"' || character == '\\' {
                rendered.push('\\');
            }
            rendered.push(character);
        }
        rendered.push('"');
    }
    rendered.push('\n');
    rendered
}

/// [`dockerfile_labels`] wrapped in the two markers.
///
/// This is the form to paste, and the only form a drift check can find again. The instruction
/// alone is enough to *build* the image; the markers are what let a later run cut the committed
/// region back out and diff it, without knowing how many labels the block had when it was written.
pub fn dockerfile_block(contract: &Contract, path: &str) -> String {
    format!(
        "{MARKER_BEGIN}\n{}{MARKER_END}\n",
        dockerfile_labels(contract, path)
    )
}

/// The committed block cut back out of a Dockerfile, markers excluded.
///
/// An absent, unterminated or empty region is refused rather than treated as "no labels": all
/// three would compare equal to nothing and report success, which is the one failure this scheme
/// cannot afford.
///
/// # Errors
/// [`Error::Invalid`] for each of those three, naming which.
pub fn committed_block(dockerfile: &str) -> Result<&str, Error> {
    let begin = dockerfile.find(MARKER_BEGIN).ok_or_else(|| {
        Error::Invalid(format!(
            "the Dockerfile carries no `{MARKER_BEGIN}` line, so the generated label block has \
             nowhere to go and nothing to be compared against. `render --format dockerfile` emits \
             the region, markers included."
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
/// [`Error::Invalid`] rather than an empty map for the two inputs that otherwise look like
/// success, because a comparison against nothing passes: `null`, which is what reading the wrong
/// JSON path yields, and a value that is not an object of strings. An empty object is *accepted*
/// and fails in the comparison instead, naming the labels that are missing — which is the more
/// useful message, and the correct place for the judgement.
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
                    "the label `{name}` is {}, and a label is a string.",
                    kind(value)
                ))),
            })
            .collect(),
        serde_json::Value::Null => Err(Error::Invalid(
            "the labels are `null`. An image with no labels reports `{}`; a `null` means the JSON \
             path was wrong, and a comparison against nothing would pass without checking a single \
             label."
                .to_owned(),
        )),
        other => Err(Error::Invalid(format!(
            "the labels are {}, and `docker inspect` reports an object.",
            kind(other)
        ))),
    }
}

/// Every label this contract expects that a built image gets wrong, in declaration order.
///
/// Empty means the image carries them all. Extra labels are ignored — an image carries
/// `org.opencontainers.image.*` and whatever its base contributed, and none of that is this
/// document's business.
///
/// Checking the **image** rather than the Dockerfile is the whole value. A source diff sees the
/// recipe: it cannot see a build argument that failed to interpolate, a label a base image
/// overrode, or a `LABEL` line deleted in a branch that was not the one diffed.
pub fn check_labels(
    contract: &Contract,
    path: &str,
    labels: &BTreeMap<String, String>,
) -> Vec<LabelFault> {
    let mut faults = Vec::new();
    for (name, expected) in contract.labels(path) {
        match labels.get(name) {
            None => faults.push(LabelFault::Missing { name }),
            Some(found) if found != &expected => faults.push(LabelFault::Wrong {
                name,
                expected,
                found: found.clone(),
            }),
            Some(_) => {}
        }
    }
    faults
}

/// One thing an image gets wrong about its own labels.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LabelFault {
    /// The image carries no such label.
    Missing {
        /// Which one.
        name: &'static str,
    },
    /// The image carries it with another value.
    Wrong {
        /// Which one.
        name: &'static str,
        /// What the contract says it should be.
        expected: String,
        /// What the image says.
        found: String,
    },
}

impl std::fmt::Display for LabelFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { name } => write!(f, "the image carries no `{name}` label"),
            Self::Wrong {
                name,
                expected,
                found,
            } => write!(
                f,
                "`{name}` is `{found}`, and the contract says `{expected}`"
            ),
        }
    }
}

/// A report over every fault, or `None` when there are none.
pub fn report(faults: &[LabelFault]) -> Option<String> {
    if faults.is_empty() {
        return None;
    }
    let mut out = String::from("the built image does not carry the contract's labels:");
    for fault in faults {
        let _ = write!(out, "\n  {fault}");
    }
    Some(out)
}

/// What a JSON value is, for a message about having read the wrong one.
const fn kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "`null`",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::{committed_block, labels_from_json};

    #[test]
    fn null_labels_are_refused_rather_than_read_as_none() {
        let error = labels_from_json("null").expect_err("`null` is the wrong JSON path");
        assert!(error.to_string().contains("path was wrong"), "{error}");
    }

    #[test]
    fn the_three_shapes_all_reach_the_same_map() {
        let bare = r#"{"a": "1"}"#;
        let docker = r#"{"Config": {"Labels": {"a": "1"}}}"#;
        let crane = r#"{"config": {"Labels": {"a": "1"}}}"#;
        let expected = labels_from_json(bare).expect("the bare object reads");
        assert_eq!(labels_from_json(docker).expect("docker inspect"), expected);
        assert_eq!(labels_from_json(crane).expect("crane config"), expected);
    }

    #[test]
    fn an_empty_region_is_refused_rather_than_compared_against_nothing() {
        let dockerfile = "# terrace-config:labels:begin\n# terrace-config:labels:end\n";
        let error = committed_block(dockerfile).expect_err("an empty region checks nothing");
        assert!(error.to_string().contains("without checking"), "{error}");
    }
}
