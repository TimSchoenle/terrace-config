//! `# @config` markers: which contract key a chart value feeds, stated beside the value.
//!
//! ```text
//! # @schema
//! # # @config projection telemetry.sentry_dsn optional
//! # type: string
//! # @schema
//! # -- Sentry DSN (`telemetry.sentry_dsn`). Empty disables Sentry entirely.
//! sentryDsn: ""
//! ```
//!
//! A contract says what the image reads. A declaration says which contract describes which rendered
//! document. Neither says which *chart value* an operator sets to move a given setting, and that is
//! the fact this gives a name to.
//!
//! # Two passes, meeting on a line number
//!
//! | Pass | Reads | Gives |
//! |---|---|---|
//! | the event parser | the structure | every mapping key's dotted path, and the line it sits on |
//! | the line scan | the comments | every marker and every `@schema` block, and their lines |
//!
//! The second pass is line-keyed and no crate can replace it: nothing in Rust preserves YAML
//! comments, and the marker *is* a comment. The first pass is what a line reader cannot do. The
//! implementation this was ported from tracked indentation by hand and carried two exceptions for
//! the regions that breaks in — sequence items, and block scalars, where a `port: 1` inside a
//! literal block looks exactly like a mapping key. To an event parser it is one scalar. Both
//! exceptions are gone, and so is the class of defect they existed to prevent.
//!
//! # Where a marker may sit, which was measured rather than chosen
//!
//! **The first line inside the value's `@schema` block, written as a YAML comment.** Two other
//! generators read the comments above a chart value and both rewrite a committed file, so a
//! placement is a claim about them rather than a matter of taste. Every other placement was
//! measured and every other placement damages one of the two — a marker on its own comment line
//! above the description puts a leading newline into the generated schema, one below it appends the
//! marker to the value's row in the generated README, and a blank line anywhere in the run loses
//! every description in the chart. So each wrong placement is refused *by name*, with what it would
//! have done.
//!
//! There is no lenient reading of a marker. A comment that was meant to be one and is not
//! understood must fail, because the alternative is a value that looks bound to a reviewer and is
//! bound to nothing.

use std::collections::BTreeMap;

use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::error::Error;

/// The comment introducer.
///
/// `@config` rather than `@contract` because a marker names the *setting* a value feeds; the
/// contract is merely the document that happens to describe it.
pub const MARKER: &str = "@config";

/// The delimiter a value's schema block is enclosed in.
///
/// The one region of a values file whose comment content is YAML rather than prose, which is what
/// makes it the one place a marker can sit without either generator seeing it.
const SCHEMA_DELIMITER: &str = "@schema";

/// What kind of thing a marker binds its value to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// One chart value feeds one contract key directly.
    Projection,
    /// A chart value feeds a container-typed key, whose sub-keys the deployment chooses.
    Structured,
    /// Several chart values are composed into one key's value by the template.
    Composed,
    /// The value feeds a declared external variable rather than a key.
    External,
}

impl Class {
    /// The spelling a marker uses.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Projection => "projection",
            Self::Structured => "structured",
            Self::Composed => "composed",
            Self::External => "external",
        }
    }

    /// The class one spelling names.
    pub fn parse(spelling: &str) -> Option<Self> {
        match spelling {
            "projection" => Some(Self::Projection),
            "structured" => Some(Self::Structured),
            "composed" => Some(Self::Composed),
            "external" => Some(Self::External),
            _ => None,
        }
    }

    /// Whether the target is a key path rather than an external variable's name.
    pub const fn targets_a_key(self) -> bool {
        !matches!(self, Self::External)
    }

    /// Every spelling, for a message that has to list them.
    pub const ALL: [Self; 4] = [
        Self::Projection,
        Self::Structured,
        Self::Composed,
        Self::External,
    ];
}

/// One `# @config` comment, and the chart value whose `@schema` block it was written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// The chart the values file belongs to.
    pub chart: String,
    /// The dotted path of the value this binds — taken from the document's own structure, so it
    /// cannot disagree with the file.
    pub values_path: String,
    /// The marker's own line, because that is the line a reader has to edit.
    pub line: usize,
    /// What kind of binding this is.
    pub class: Class,
    /// The documents this binding is scoped to, or [`None`] for every document whose contract
    /// declares the target.
    ///
    /// The default is the common case rather than the ambiguous one: a chart writes one value into
    /// every service that reads it, so one line of a template can reach eight documents. A scope is
    /// written only where the default would over-claim.
    pub documents: Option<Vec<String>>,
    /// The key path or external variable name this value feeds.
    pub target: String,
    /// Whether the key may legitimately be absent when the value is empty.
    pub optional: bool,
    /// A values path that must be switched on before this value reaches the document at all.
    pub condition: Option<String>,
}

impl Marker {
    /// `chart/values.yaml:LINE`, for a message a reader can jump to.
    pub fn at(&self) -> String {
        format!("{}/values.yaml:{}", self.chart, self.line)
    }
}

/// One `@schema` block, and the chart value it belongs to.
///
/// `lines` are the raw lines *between* the delimiters, marker run and schema alike, exactly as they
/// appear in the file. `start` and `end` are the delimiters' own line numbers, so a caller
/// rewriting a block replaces `start + 1 ..= end - 1` and leaves everything else alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The chart the values file belongs to.
    pub chart: String,
    /// The value the block describes.
    pub values_path: String,
    /// The opening delimiter's line.
    pub start: usize,
    /// The closing delimiter's line.
    pub end: usize,
    /// The delimiter's indentation, which the rewritten body has to match.
    pub indent: usize,
    /// The lines between the delimiters.
    pub lines: Vec<String>,
}

/// Every marker and every `@schema` block in one values file, bound to their values.
///
/// Every problem is collected before any is raised, which is the posture every rule here takes: one
/// broken line must not hide the state of the rest.
///
/// # Errors
/// [`Error::Invalid`] listing every marker that cannot be read as one, or that binds nothing.
pub fn read(text: &str, chart: &str) -> Result<(Vec<Marker>, Vec<Block>), Error> {
    let index = Index::of(text)?;
    Scan::new(chart, &index).run(text)
}

// ------------------------------------------------------------------------------------------
// Pass 1 — the structure
// ------------------------------------------------------------------------------------------

/// Where every mapping key is, and which lines are inside a scalar rather than being one.
#[derive(Debug, Default)]
struct Index {
    /// Line number to the dotted path of the mapping key on it.
    keys: BTreeMap<usize, String>,
    /// Lines covered by a multi-line scalar's own text. A `port: 1` in a literal block lives here,
    /// and is not a key however much it looks like one.
    inside_a_scalar: Vec<(usize, usize)>,
}

impl Index {
    fn of(text: &str) -> Result<Self, Error> {
        let mut sink = Walk::default();
        Parser::new_from_str(text)
            .load(&mut sink, true)
            .map_err(|failure| Error::Invalid(format!("is not valid YAML: {failure}")))?;
        Ok(sink.index)
    }

    /// The dotted path of the value on this line, if it carries a mapping key.
    fn key_at(&self, line: usize) -> Option<&str> {
        self.keys.get(&line).map(String::as_str)
    }

    /// Whether this line is a multi-line scalar's own text rather than structure.
    ///
    /// Half-open: the span's end is the line *after* the scalar's last, and its start is the
    /// scalar's first content line — the mapping key that introduced it sits on the line above and
    /// is a key like any other.
    fn buried(&self, line: usize) -> bool {
        self.inside_a_scalar
            .iter()
            .any(|(start, end)| line >= *start && line < *end)
    }
}

/// One level of the document, while the events are being walked.
enum Level {
    Map {
        key: Option<String>,
        expecting_key: bool,
    },
    Seq,
}

/// Builds the [`Index`] from the parser's events.
#[derive(Default)]
struct Walk {
    levels: Vec<Level>,
    index: Index,
}

impl Walk {
    /// The dotted path of whatever the walk is currently inside.
    ///
    /// Sequence indices are left out: a marker cannot bind to a sequence item — the value it
    /// describes is a mapping value — so a path through one is a path no rule can use.
    fn dotted(&self) -> Option<String> {
        let mut parts: Vec<&str> = Vec::new();
        for level in &self.levels {
            match level {
                Level::Map { key: Some(key), .. } => parts.push(key),
                Level::Map { key: None, .. } => {}
                Level::Seq => return None,
            }
        }
        Some(parts.join("."))
    }

    /// One value has been read, so the container it was in expects the next thing.
    fn advance(&mut self) {
        if let Some(Level::Map { expecting_key, .. }) = self.levels.last_mut() {
            *expecting_key = true;
        }
    }
}

impl<'a> SpannedEventReceiver<'a> for Walk {
    fn on_event(&mut self, event: Event<'a>, span: Span) {
        match event {
            Event::MappingStart(..) => self.levels.push(Level::Map {
                key: None,
                expecting_key: true,
            }),
            Event::SequenceStart(..) => self.levels.push(Level::Seq),
            Event::MappingEnd | Event::SequenceEnd => {
                self.levels.pop();
                self.advance();
            }
            Event::Scalar(value, ..) => {
                let start = span.start.line();
                let end = span.end.line();
                if end > start {
                    self.index.inside_a_scalar.push((start, end));
                }
                let expecting = matches!(
                    self.levels.last(),
                    Some(Level::Map {
                        expecting_key: true,
                        ..
                    })
                );
                if expecting {
                    if let Some(Level::Map { key, expecting_key }) = self.levels.last_mut() {
                        *key = Some(value.to_string());
                        *expecting_key = false;
                    }
                    if let Some(dotted) = self.dotted() {
                        self.index.keys.insert(start, dotted);
                    }
                } else {
                    self.advance();
                }
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------------------------------
// Pass 2 — the comments
// ------------------------------------------------------------------------------------------

/// The line scan, and the state machine that binds a block to the value below it.
struct Scan<'a> {
    chart: &'a str,
    index: &'a Index,
    problems: Vec<String>,
    markers: Vec<Marker>,
    blocks: Vec<Block>,
    /// True between the delimiters.
    in_schema: bool,
    /// True while still inside the block's opening run of marker lines.
    in_marker_run: bool,
    /// Markers read out of a block, waiting for the value that block belongs to.
    pending: Vec<(usize, Parsed)>,
    /// The block they came out of, waiting for the same value.
    pending_block: Option<(usize, usize, usize, Vec<String>)>,
    /// The block currently open: its start, its indent, and the lines read so far.
    open_block: Option<(usize, usize, Vec<String>)>,
}

/// One marker's body, read into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Parsed {
    class: Class,
    documents: Option<Vec<String>>,
    target: String,
    optional: bool,
    condition: Option<String>,
}

impl<'a> Scan<'a> {
    fn new(chart: &'a str, index: &'a Index) -> Self {
        Self {
            chart,
            index,
            problems: Vec::new(),
            markers: Vec::new(),
            blocks: Vec::new(),
            in_schema: false,
            in_marker_run: false,
            pending: Vec::new(),
            pending_block: None,
            open_block: None,
        }
    }

    fn refuse(&mut self, line: usize, what: impl std::fmt::Display) {
        self.problems
            .push(format!("{}/values.yaml:{line}: {what}", self.chart));
    }

    /// Markers whose block never reached a value bind nothing. Say so, naming both.
    fn strand(&mut self, what: &str) {
        let stranded: Vec<usize> = self.pending.iter().map(|(line, _)| *line).collect();
        for line in stranded {
            self.refuse(
                line,
                format!(
                    "this `{MARKER}` marker's `@schema` block is followed by {what} rather than by \
                     the value it binds, so it binds nothing"
                ),
            );
        }
        self.pending.clear();
        // A block that never reached a value describes nothing either, and unlike a stranded marker
        // that is ordinary: `# @schema` above a comment-only section header is how several charts
        // open a subtree. Dropped in silence for that reason.
        self.pending_block = None;
    }

    fn run(mut self, text: &str) -> Result<(Vec<Marker>, Vec<Block>), Error> {
        for (offset, raw) in text.lines().enumerate() {
            let number = offset + 1;
            if self.index.buried(number) {
                continue;
            }
            let line = raw.trim_end();

            if line.trim().is_empty() {
                // A blank line ends the comment run, which is exactly how both generators lose the
                // description too.
                if !self.in_schema {
                    self.strand("a blank line");
                }
                continue;
            }

            let indent = line.len() - line.trim_start().len();
            let (code, comment) = split_comment(line);

            if is_delimiter(line) {
                self.delimiter(number, indent);
                continue;
            }

            if self.in_schema {
                self.inside_block(number, line, comment.as_deref());
                continue;
            }

            if code.trim().is_empty() {
                if is_marker(comment.as_deref()) {
                    self.refuse(
                        number,
                        format!(
                            "this `{MARKER}` marker is a comment line of its own, outside the \
                             `@schema` delimiters. Both generators read that run as the value's \
                             description: above the `# --` the schema generator emits a leading \
                             newline into the generated schema, and below it the README generator \
                             appends the marker to the value's row. Write it as `# # {MARKER} ...` \
                             on the first line inside the block instead"
                        ),
                    );
                }
                continue;
            }

            self.value(number, &code, comment.as_deref());
        }

        self.strand("the end of the file");

        if self.problems.is_empty() {
            Ok((self.markers, self.blocks))
        } else {
            Err(Error::Invalid(self.problems.join("\n")))
        }
    }

    /// A `# @schema` line: the block opens or closes.
    fn delimiter(&mut self, number: usize, indent: usize) {
        if self.in_schema {
            self.in_schema = false;
            self.in_marker_run = false;
            if let Some((start, block_indent, collected)) = self.open_block.take() {
                self.pending_block = Some((start, number, block_indent, collected));
            }
        } else {
            self.strand("a second `@schema` block");
            self.in_schema = true;
            self.in_marker_run = true;
            self.open_block = Some((number, indent, Vec::new()));
        }
    }

    /// One line between the delimiters.
    fn inside_block(&mut self, number: usize, line: &str, comment: Option<&str>) {
        if let Some((_, _, collected)) = self.open_block.as_mut() {
            collected.push(line.to_owned());
        }

        let nested = schema_comment(comment);
        if is_marker(nested.as_deref()) {
            if !self.in_marker_run {
                self.refuse(
                    number,
                    format!(
                        "this `{MARKER}` marker is below the schema rather than above it. A \
                         value's markers are the run of lines directly inside the opening \
                         `# @schema`, so that a reader looking for what a value binds reads the \
                         top of one block and stops"
                    ),
                );
                return;
            }
            let body = nested
                .as_deref()
                .and_then(|nested| nested.strip_prefix(MARKER))
                .unwrap_or_default()
                .trim();
            match parse_marker(body) {
                Err(failure) => self.refuse(number, failure),
                Ok(parsed) => {
                    let duplicate = self.pending.iter().find(|(_, other)| {
                        (&other.documents, &other.target) == (&parsed.documents, &parsed.target)
                    });
                    if let Some((first, _)) = duplicate {
                        let first = *first;
                        self.refuse(
                            number,
                            format!(
                                "this value already binds '{}' on line {first}; one binding said \
                                 twice is one of the two being out of date",
                                parsed.target
                            ),
                        );
                    } else {
                        self.pending.push((number, parsed));
                    }
                }
            }
            return;
        }

        if is_marker(comment) {
            self.refuse(
                number,
                format!(
                    "this `{MARKER}` marker is schema *content* rather than a comment on it, and \
                     `@` is a reserved indicator in YAML, so the schema generator fails on the \
                     block outright. Write it as a comment within the schema: `# # {MARKER} ...`"
                ),
            );
        }
        self.in_marker_run = false;
    }

    /// A line carrying something other than a comment: the value a pending block binds to, or not.
    fn value(&mut self, number: usize, code: &str, comment: Option<&str>) {
        let Some(values_path) = self.index.key_at(number).map(str::to_owned) else {
            let what = if is_sequence_item(code) {
                "a sequence item"
            } else {
                "a line this parser cannot read as a value"
            };
            self.strand(what);
            if is_marker(comment) {
                self.refuse(
                    number,
                    format!(
                        "this `{MARKER}` marker sits on {what}; a marker describes one mapping \
                         value and is written inside that value's `@schema` block"
                    ),
                );
            }
            return;
        };

        if is_marker(comment) {
            self.refuse(
                number,
                format!(
                    "this `{MARKER}` marker is the value's trailing comment. Both generators \
                     ignore it there, so this is a convention rather than a measurement: every \
                     other piece of metadata about a value lives above it, and the marker's place \
                     is the first line inside the `@schema` block, as `# # {MARKER} ...`"
                ),
            );
        }

        if let Some((start, end, indent, lines)) = self.pending_block.take() {
            self.blocks.push(Block {
                chart: self.chart.to_owned(),
                values_path: values_path.clone(),
                start,
                end,
                indent,
                lines,
            });
        }
        for (line, parsed) in std::mem::take(&mut self.pending) {
            self.markers.push(Marker {
                chart: self.chart.to_owned(),
                values_path: values_path.clone(),
                line,
                class: parsed.class,
                documents: parsed.documents,
                target: parsed.target,
                optional: parsed.optional,
                condition: parsed.condition,
            });
        }
    }
}

/// Split one line into its YAML and its trailing comment, respecting quotes.
///
/// A `#` inside a quoted scalar is not a comment, and splitting on the first one would truncate the
/// value and then fail to find the mapping key it was reading.
fn split_comment(line: &str) -> (String, Option<String>) {
    let characters: Vec<char> = line.chars().collect();
    let mut quote: Option<char> = None;
    for (index, character) in characters.iter().enumerate() {
        // Inside a quoted scalar nothing is a comment, which is the whole reason this is not a
        // `split_once('#')`: truncating there loses the value, and then loses the key with it.
        if let Some(open) = quote {
            if *character == open {
                quote = None;
            }
            continue;
        }
        match *character {
            '"' | '\'' => quote = Some(*character),
            // A `#` is only an introducer at the start of a line or after whitespace, which is
            // what keeps a colour like `#fff` from ending the value that holds it.
            '#' if index == 0 || characters[index - 1] == ' ' || characters[index - 1] == '\t' => {
                let comment: String = characters[index + 1..].iter().collect();
                return (
                    characters[..index].iter().collect(),
                    Some(comment.trim().to_owned()),
                );
            }
            _ => {}
        }
    }
    (line.to_owned(), None)
}

/// Whether one line inside a block opens with a marker rather than with schema.
///
/// The split a caller regenerating a block needs: the marker run is hand-written and none of the
/// generator's business, and everything after it is what a regeneration replaces.
pub fn opens_with_a_marker(line: &str) -> bool {
    let (_, comment) = split_comment(line);
    is_marker(schema_comment(comment.as_deref()).as_deref())
}

/// Whether a comment body is a marker, and not merely a word beginning the same way.
fn is_marker(comment: Option<&str>) -> bool {
    comment.is_some_and(|comment| comment == MARKER || comment.starts_with(&format!("{MARKER} ")))
}

/// The comment carried by one line *inside* a block, if it carries one.
///
/// Inside the delimiters the text after `# ` is the schema, parsed as YAML, so a marker written
/// there is a comment within that YAML and the line reads `# # @config ...`. This returns the inner
/// comment for such a line, and [`None`] for a line whose content is real schema.
fn schema_comment(comment: Option<&str>) -> Option<String> {
    comment
        .filter(|comment| comment.starts_with('#'))
        .map(|comment| comment[1..].trim().to_owned())
}

/// Whether a line opens the schema block.
fn is_delimiter(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed
        .strip_prefix('#')
        .is_some_and(|rest| rest.trim() == SCHEMA_DELIMITER)
}

/// Whether a line is a sequence item, which is not a value a marker may describe.
fn is_sequence_item(code: &str) -> bool {
    let trimmed = code.trim_start();
    trimmed == "-" || trimmed.starts_with("- ")
}

/// Whether a name is a dotted values path and nothing else.
fn is_values_path(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|held| held.is_ascii_alphanumeric() || matches!(held, '_' | '.' | '-'))
        && name
            .chars()
            .next()
            .is_some_and(|held| held.is_ascii_alphanumeric() || held == '_')
}

/// Read one marker's body — everything after `@config` — into its parts.
fn parse_marker(text: &str) -> Result<Parsed, String> {
    let mut tokens = text.split_whitespace();
    let Some(spelling) = tokens.next() else {
        return Err(format!(
            "`{MARKER}` names nothing; expected `{MARKER} <class> <target>`"
        ));
    };
    let Some(class) = Class::parse(spelling) else {
        return Err(format!(
            "unknown class '{spelling}'; known classes are {}",
            Class::ALL
                .iter()
                .map(|held| held.label())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };

    let Some(target) = tokens.next() else {
        return Err(format!("`{MARKER} {spelling}` names no target"));
    };
    let (documents, target) = match target.split_once(':') {
        Some((scope, name)) => (Some(scope), name),
        None => (None, target),
    };
    if !is_values_path(target) || documents.is_some_and(str::is_empty) {
        return Err(
            "a target is `<key.path>`, or `<document>[,<document>...]:<key.path>` to scope it, in \
             the contract's own spelling"
                .to_owned(),
        );
    }
    let documents = match documents {
        None => None,
        Some(scope) => {
            let named: Vec<String> = scope.split(',').map(str::to_owned).collect();
            if named.iter().any(|name| !is_values_path(name)) {
                return Err(
                    "a target is `<key.path>`, or `<document>[,<document>...]:<key.path>` to \
                     scope it, in the contract's own spelling"
                        .to_owned(),
                );
            }
            let mut unique = named.clone();
            unique.sort();
            unique.dedup();
            if unique.len() != named.len() {
                return Err(format!("the scope '{scope}' names a document twice"));
            }
            Some(named)
        }
    };

    let mut rest: Vec<&str> = tokens.collect();
    let optional = rest.first() == Some(&"optional");
    if optional {
        rest.remove(0);
    }

    let mut condition = None;
    if rest.first() == Some(&"when") {
        rest.remove(0);
        let Some(named) = rest.first().copied() else {
            return Err("`when` names no values path".to_owned());
        };
        rest.remove(0);
        if !is_values_path(named) {
            return Err(format!("`when {named}` is not a dotted values path"));
        }
        condition = Some(named.to_owned());
    }

    if !rest.is_empty() {
        // Including `when ... optional`, the other order. One relationship has one spelling;
        // accepting both would make two files that wrote it differently look like they said
        // different things.
        return Err(format!(
            "unexpected '{}'; the form is `{MARKER} <class> <target> [optional] [when \
             <values-path>]`, in that order",
            rest.join(" ")
        ));
    }

    Ok(Parsed {
        class,
        documents,
        target: target.to_owned(),
        optional,
        condition,
    })
}

#[cfg(test)]
mod tests {
    use super::{Class, read};

    fn markers(text: &str) -> Vec<super::Marker> {
        read(text, "x").expect("the markers read").0
    }

    fn failure(text: &str) -> String {
        read(text, "x")
            .expect_err("the markers do not read")
            .to_string()
    }

    #[test]
    fn a_marker_binds_the_value_its_block_belongs_to() {
        let found = markers(
            "telemetry:\n  # @schema\n  # # @config projection telemetry.sentry_dsn optional\n  \
             # type: string\n  # @schema\n  # -- Sentry DSN.\n  sentryDsn: \"\"\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].values_path, "telemetry.sentryDsn");
        assert_eq!(found[0].target, "telemetry.sentry_dsn");
        assert_eq!(found[0].class, Class::Projection);
        assert!(found[0].optional);
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn the_values_path_comes_from_the_structure_rather_than_from_indentation() {
        // Two levels down, under a key whose own value is a mapping. A line reader has to track
        // this; the parser already knows it.
        let found = markers(
            "csp:\n  cloudflare:\n    # @schema\n    # # @config projection csp.cf.nonce\n    \
             # type: string\n    # @schema\n    scriptNonce: \"\"\n",
        );
        assert_eq!(found[0].values_path, "csp.cloudflare.scriptNonce");
    }

    #[test]
    fn a_mapping_key_inside_a_block_scalar_is_not_a_value() {
        // The exception a line reader needs a special case for. `port: 1` here is text.
        let found = markers(
            "literal: |\n  not: a mapping\n  port: 1\n# @schema\n# # @config projection a.b\n\
             # type: string\n# @schema\nafter: \"\"\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].values_path, "after");
    }

    #[test]
    fn a_scope_narrows_the_binding_and_may_not_name_a_document_twice() {
        let found = markers(
            "# @schema\n# # @config projection api,worker:server.port\n# type: integer\n\
             # @schema\nport: 1\n",
        );
        assert_eq!(
            found[0].documents.as_deref(),
            Some(["api".to_owned(), "worker".to_owned()].as_slice())
        );
        assert_eq!(found[0].target, "server.port");

        assert!(
            failure(
                "# @schema\n# # @config projection api,api:server.port\n# type: integer\n\
                 # @schema\nport: 1\n"
            )
            .contains("names a document twice")
        );
    }

    #[test]
    fn a_condition_names_the_gate_the_value_is_behind() {
        let found = markers(
            "# @schema\n# # @config projection seed.email when bootstrap.seedAdmin.enabled\n\
             # type: string\n# @schema\nemail: \"\"\n",
        );
        assert_eq!(
            found[0].condition.as_deref(),
            Some("bootstrap.seedAdmin.enabled")
        );
    }

    #[test]
    fn the_two_modifiers_have_one_order() {
        // One relationship has one spelling; accepting both orders would make two files that wrote
        // it differently look like they said different things.
        assert!(
            failure(
                "# @schema\n# # @config projection a.b when c optional\n# type: string\n# @schema\n\
             a: \"\"\n"
            )
            .contains("in that order")
        );
    }

    #[test]
    fn an_unreadable_marker_fails_rather_than_binding_nothing_quietly() {
        assert!(
            failure("# @schema\n# # @config\n# type: string\n# @schema\na: 1\n")
                .contains("names nothing")
        );
        assert!(
            failure("# @schema\n# # @config guess a.b\n# type: string\n# @schema\na: 1\n")
                .contains("unknown class 'guess'")
        );
        assert!(
            failure("# @schema\n# # @config projection\n# type: string\n# @schema\na: 1\n")
                .contains("names no target")
        );
    }

    #[test]
    fn a_marker_below_the_schema_is_refused_by_name() {
        assert!(
            failure("# @schema\n# type: string\n# # @config projection a.b\n# @schema\na: \"\"\n")
                .contains("below the schema rather than above it")
        );
    }

    #[test]
    fn a_marker_written_as_schema_content_is_refused_with_the_spelling_that_works() {
        assert!(
            failure("# @schema\n# @config projection a.b\n# type: string\n# @schema\na: \"\"\n")
                .contains("reserved indicator in YAML")
        );
    }

    #[test]
    fn a_marker_on_its_own_comment_line_names_what_it_would_have_broken() {
        let message = failure("# @config projection a.b\n# -- The value.\na: \"\"\n");
        assert!(
            message.contains("outside the `@schema` delimiters"),
            "{message}"
        );
        assert!(message.contains("leading newline"), "{message}");
    }

    #[test]
    fn a_marker_as_a_trailing_comment_is_refused() {
        assert!(
            failure("a: \"\" # @config projection a.b").contains("the value's trailing comment")
        );
    }

    #[test]
    fn a_blank_line_between_the_block_and_the_value_strands_the_marker() {
        // The placement that loses every description in the chart, so it is refused rather than
        // silently bound to whatever came next.
        assert!(
            failure(
                "# @schema\n# # @config projection a.b\n# type: string\n# @schema\n\na: \"\"\n"
            )
            .contains("binds nothing")
        );
    }

    #[test]
    fn a_marker_whose_block_is_followed_by_a_sequence_item_binds_nothing() {
        assert!(
            failure(
                "items:\n  # @schema\n  # # @config projection a.b\n  # type: string\n  \
                     # @schema\n  - one\n"
            )
            .contains("binds nothing")
        );
    }

    #[test]
    fn one_binding_said_twice_is_one_of_the_two_being_out_of_date() {
        assert!(
            failure(
                "# @schema\n# # @config projection a.b\n# # @config projection a.b\n\
                 # type: string\n# @schema\na: \"\"\n"
            )
            .contains("already binds")
        );
    }

    #[test]
    fn two_different_bindings_on_one_value_are_ordinary() {
        // A composed value legitimately feeds more than one key.
        let found = markers(
            "# @schema\n# # @config composed a.b\n# # @config composed a.c\n# type: string\n\
             # @schema\na: \"\"\n",
        );
        assert_eq!(found.len(), 2, "{found:?}");
    }

    #[test]
    fn a_hash_inside_a_quoted_value_is_not_a_comment() {
        // Splitting on the first `#` would truncate the value and then fail to find the key.
        let found = markers(
            "# @schema\n# # @config projection a.b\n# type: string\n# @schema\ncolour: \"#fff\"\n",
        );
        assert_eq!(found[0].values_path, "colour");
    }

    #[test]
    fn a_block_carries_its_delimiters_and_its_body() {
        let (_, blocks) = read(
            "# @schema\n# # @config projection a.b\n# type: string\n# @schema\na: \"\"\n",
            "x",
        )
        .expect("it reads");
        assert_eq!(blocks.len(), 1);
        assert_eq!((blocks[0].start, blocks[0].end), (1, 4));
        assert_eq!(blocks[0].values_path, "a");
        assert_eq!(blocks[0].lines.len(), 2);
    }

    #[test]
    fn every_problem_is_collected_before_any_is_raised() {
        let message = failure(
            "# @schema\n# # @config guess a.b\n# @schema\na: 1\n\
             # @schema\n# # @config projection\n# @schema\nb: 2\n",
        );
        assert_eq!(message.lines().count(), 2, "{message}");
    }
}
