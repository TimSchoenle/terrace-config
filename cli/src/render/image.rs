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

/// Most marked regions one Dockerfile may carry.
///
/// A region per published image is the shape this exists for. Past this the file is generated or
/// hostile, and each region would become its own finding, so the file is refused as a whole.
pub const MAX_REGIONS: usize = 32;

/// One marked region of a Dockerfile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region<'a> {
    /// 1-based line of the opening marker, so a report names the region that is wrong.
    pub line: usize,
    /// What sits between the markers, markers excluded, outer blank lines trimmed.
    pub block: &'a str,
}

/// Every committed block cut back out of a Dockerfile, in file order.
///
/// A Dockerfile may build several images, one runtime stage each, and every stage that publishes
/// this contract carries its own region. All of them are returned, so each is compared; refusing a
/// second region would leave the only arrangement in which the check matters unable to use it.
///
/// A marker is a line that is exactly the marker, trailing whitespace aside. A closing marker with
/// nothing open is ignored: the opening marker defines a region. An empty region is *returned*,
/// with an empty `block`, so the caller reports it against its line while still comparing the
/// others.
///
/// # Errors
/// [`Error::Invalid`] when the file has no region, opens one inside another, leaves one
/// unterminated, or carries more than [`MAX_REGIONS`]. Each would otherwise compare equal to nothing
/// and pass, or leave where one region ends undecided.
pub fn committed_blocks(dockerfile: &str) -> Result<Vec<Region<'_>>, Error> {
    Ok(spans(dockerfile)?
        .into_iter()
        .map(|span| Region {
            line: span.line,
            block: dockerfile[span.body].trim_matches(['\n', '\r']),
        })
        .collect())
}

/// The Dockerfile with every marked region's body replaced by this document's `LABEL` instruction.
///
/// The markers stay exactly as written, and so does everything outside them, so the only diff a
/// rewrite ever produces is inside a region. An empty region is filled, which is how a block is
/// placed in the first place: the author writes the two markers where the labels belong, once, and
/// this writes what goes between them. The file's line ending is kept — a Dockerfile checked out
/// with CRLF gets a CRLF block — judged from its first line.
///
/// # Errors
/// As [`committed_blocks`]: a file with no region is refused rather than given one, because where
/// the labels belong is the author's decision — which stage, above or below which instruction.
pub fn rewrite_blocks(dockerfile: &str, contract: &Contract, path: &str) -> Result<String, Error> {
    let newline = if dockerfile
        .split_inclusive('\n')
        .next()
        .is_some_and(|first| first.ends_with("\r\n"))
    {
        "\r\n"
    } else {
        "\n"
    };
    let block = dockerfile_labels(contract, path).replace('\n', newline);

    let mut rewritten = String::with_capacity(dockerfile.len() + block.len());
    let mut copied = 0;
    for span in spans(dockerfile)? {
        rewritten.push_str(&dockerfile[copied..span.body.start]);
        // A region is closed by a marker on a later line, so the opening marker always ends in a
        // newline and the body starts on a line of its own.
        rewritten.push_str(&block);
        copied = span.body.end;
    }
    rewritten.push_str(&dockerfile[copied..]);
    Ok(rewritten)
}

/// One marked region, by position: its opening marker's line, and the bytes between the markers.
struct Span {
    /// 1-based line of the opening marker.
    line: usize,
    /// From the start of the line after the opening marker to the start of the closing marker.
    body: std::ops::Range<usize>,
}

/// Every marked region of a Dockerfile, the one reading [`committed_blocks`] and [`rewrite_blocks`]
/// share, so the region a check compares is the region a rewrite replaces.
fn spans(dockerfile: &str) -> Result<Vec<Span>, Error> {
    let mut spans = Vec::new();
    let mut open: Option<(usize, usize)> = None;
    let mut offset = 0;

    for (index, raw) in dockerfile.split_inclusive('\n').enumerate() {
        let line = raw.trim_end();
        if line == MARKER_BEGIN {
            if open.is_some() {
                return Err(Error::Invalid(format!(
                    "line {}: a `{MARKER_BEGIN}` region opens inside another, so where one region \
                     ends and the next begins is undecided.",
                    index + 1
                )));
            }
            open = Some((index + 1, offset + raw.len()));
        } else if line == MARKER_END
            && let Some((line_number, body_start)) = open.take()
        {
            spans.push(Span {
                line: line_number,
                body: body_start..offset,
            });
        }
        offset += raw.len();
    }

    if let Some((line_number, _)) = open {
        return Err(Error::Invalid(format!(
            "line {line_number}: the Dockerfile opens a `{MARKER_BEGIN}` region and never closes \
             it with `{MARKER_END}`."
        )));
    }
    if spans.is_empty() {
        return Err(Error::Invalid(format!(
            "the Dockerfile carries no `{MARKER_BEGIN}` line, so the generated label block has \
             nowhere to go and nothing to be compared against. Write the two marker lines where the \
             labels belong — `{MARKER_BEGIN}` and `{MARKER_END}` — and `render --format \
             dockerfile --write` fills them."
        )));
    }
    if spans.len() > MAX_REGIONS {
        return Err(Error::Invalid(format!(
            "the Dockerfile carries {} marked regions, more than the {MAX_REGIONS} any Dockerfile \
             builds.",
            spans.len()
        )));
    }
    Ok(spans)
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
    use super::{
        MAX_REGIONS, Region, committed_blocks, dockerfile_labels, labels_from_json, rewrite_blocks,
    };
    use crate::document::Contract;

    const PATH: &str = "/config/contract.json";

    fn contract() -> Contract {
        Contract::from_json(include_str!("../../tests/fixtures/api.json"))
            .expect("the fixture reads")
    }

    #[test]
    fn a_rewrite_fills_an_empty_region_and_touches_nothing_outside_it() {
        let dockerfile =
            "FROM a\n# terrace-config:labels:begin\n# terrace-config:labels:end\nUSER 1\n";
        let rewritten = rewrite_blocks(dockerfile, &contract(), PATH).expect("one region");
        assert_eq!(
            rewritten,
            format!(
                "FROM a\n# terrace-config:labels:begin\n{}# terrace-config:labels:end\nUSER 1\n",
                dockerfile_labels(&contract(), PATH)
            )
        );
        let regions = committed_blocks(&rewritten).expect("still one region");
        assert_eq!(
            regions[0].block,
            dockerfile_labels(&contract(), PATH).trim_end()
        );
    }

    #[test]
    fn a_rewrite_replaces_every_stale_region_and_is_idempotent() {
        let dockerfile = "FROM a AS one\n# terrace-config:labels:begin\nLABEL old=\"1\"\n\n# terrace-config:labels:end\n\
                          FROM b AS two\n# terrace-config:labels:begin\n# terrace-config:labels:end\n";
        let once = rewrite_blocks(dockerfile, &contract(), PATH).expect("two regions");
        let twice = rewrite_blocks(&once, &contract(), PATH).expect("still two regions");
        assert_eq!(once, twice, "a current file is left exactly as it is");
        assert!(!once.contains("old="), "{once}");
        assert_eq!(committed_blocks(&once).expect("two").len(), 2);
    }

    #[test]
    fn a_rewrite_keeps_a_crlf_file_crlf() {
        let dockerfile =
            "FROM a\r\n# terrace-config:labels:begin\r\n# terrace-config:labels:end\r\n";
        let rewritten = rewrite_blocks(dockerfile, &contract(), PATH).expect("one region");
        assert!(
            !rewritten.replace("\r\n", "").contains('\n'),
            "every line ends CRLF: {rewritten:?}"
        );
    }

    #[test]
    fn a_rewrite_refuses_a_file_with_no_region_rather_than_choosing_a_place() {
        let error = rewrite_blocks("FROM a\n", &contract(), PATH).expect_err("no region");
        assert!(error.to_string().contains("--write` fills them"), "{error}");
    }

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
    fn an_empty_region_is_returned_so_it_is_reported_on_its_own_line() {
        let dockerfile = "FROM a\n# terrace-config:labels:begin\n# terrace-config:labels:end\n";
        let regions =
            committed_blocks(dockerfile).expect("an empty region is a finding, not a refusal");
        assert_eq!(regions, vec![Region { line: 2, block: "" }]);
    }

    #[test]
    fn every_region_is_cut_in_file_order() {
        let dockerfile = "FROM a AS one\n# terrace-config:labels:begin\nLABEL a=\"1\"\n# terrace-config:labels:end\n\
                          FROM b AS two\n# terrace-config:labels:begin  \r\nLABEL b=\"2\"\r\n# terrace-config:labels:end\r\n";
        let regions = committed_blocks(dockerfile).expect("two well-formed regions");
        assert_eq!(regions.len(), 2);
        assert_eq!(
            regions[0],
            Region {
                line: 2,
                block: "LABEL a=\"1\""
            }
        );
        assert_eq!(regions[1].line, 6);
        assert_eq!(regions[1].block, "LABEL b=\"2\"");
    }

    #[test]
    fn a_missing_region_is_refused() {
        let error = committed_blocks("FROM a\n").expect_err("no region");
        assert!(error.to_string().contains("no `"), "{error}");
    }

    #[test]
    fn a_stray_end_marker_defines_no_region() {
        let error = committed_blocks("# terrace-config:labels:end\n").expect_err("no region");
        assert!(error.to_string().contains("no `"), "{error}");
    }

    #[test]
    fn a_nested_region_is_refused() {
        let dockerfile = "# terrace-config:labels:begin\n# terrace-config:labels:begin\n# terrace-config:labels:end\n";
        let error = committed_blocks(dockerfile).expect_err("nested");
        assert!(error.to_string().contains("inside another"), "{error}");
    }

    #[test]
    fn an_unterminated_region_is_refused() {
        let error =
            committed_blocks("# terrace-config:labels:begin\nLABEL a=\"1\"\n").expect_err("open");
        assert!(error.to_string().contains("never closes"), "{error}");
    }

    #[test]
    fn an_indented_marker_is_not_a_marker() {
        let error =
            committed_blocks("  # terrace-config:labels:begin\n# terrace-config:labels:end\n")
                .expect_err("indented begin is not a marker");
        assert!(error.to_string().contains("no `"), "{error}");
    }

    #[test]
    fn more_regions_than_any_dockerfile_builds_are_refused() {
        let region = "# terrace-config:labels:begin\nLABEL a=\"1\"\n# terrace-config:labels:end\n";
        let error = committed_blocks(&region.repeat(MAX_REGIONS + 1)).expect_err("too many");
        assert!(error.to_string().contains("more than"), "{error}");
        assert_eq!(
            committed_blocks(&region.repeat(MAX_REGIONS))
                .map(|r| r.len())
                .ok(),
            Some(MAX_REGIONS)
        );
    }
}
