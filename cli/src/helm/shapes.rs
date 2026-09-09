//! Deriving a chart value's `@schema` block from the element schema its image publishes.
//!
//! A contract states a key's type as a JSON Schema object, and until `schema_version: 2` that
//! object was flat: no way to say what one *element* of a container held. A `Vec<RouteConfig>`
//! arrived as `{"type": "array"}`, which was all the producer could say, and a chart that wanted an
//! operator's editor to catch a misspelt field before the service did had to transcribe the struct
//! by hand — a copy of a fact somebody else owns, and a copy nothing regenerates is the one that
//! goes stale.
//!
//! `schema_version: 2` publishes that shape. This is what turns it into the block the chart ships.
//!
//! # Generated is the default, and the markers are the departures from it
//!
//! **Every value carrying a `projection`, `structured` or `external` binding has its block written
//! from the contract.** Nothing enrols it: the marker naming the key is the enrolment, because a
//! value that says which key it feeds has said everything a generator needs, and a second marker
//! repeating "yes, really" is one more thing to forget.
//!
//! It was the other way round once, and the cost was measured rather than argued: nine values were
//! enrolled and 185 were not, while 114 of the 185 carried a block byte-identical to the one their
//! contract describes — hand copies that happened to still be right, with nothing holding them
//! there. The other 71 differed, and every one of those differences was invisible.
//!
//! Three markers remain, and each says the *opposite* of what enrolment used to: not "derive this",
//! but "here is where the chart departs from what was derived".
//!
//! ```text
//! # @config-shape <values-path> handwritten <release> <source>
//! # @config-shape-except <values-path> <sub-path> <reason>
//! # @config-shape-narrow <values-path> <sub-path> <reason>
//! ```
//!
//! Written as plain comments *above* the block and separated from it by nothing but comments — the
//! placement measured to be invisible to both generators, and the reason a `@config` marker cannot
//! use it: inside the delimiters the text *is* the schema.
//!
//! `handwritten` is the escape hatch generation is refused for, and two things earn it: a container
//! whose element the producer has not described, where a block built from `{"type": "array"}` alone
//! would type-check nothing, and a key the producer publishes no constraint for at all, where the
//! derived block would name all six JSON types and check nothing.

use std::collections::BTreeMap;

use serde_json::{Map, Value as Json};

use crate::error::Error;
use crate::union::Merged;

use super::declaration::Bound;
use super::markers::{Block, Class, MARKER, Marker};

/// The mode a marker still declares. Generation is what every bound value gets without declaring.
pub const HANDWRITTEN: &str = "handwritten";
/// The word the retired enrolment marker used, kept so that it can be refused *by name*.
const GENERATED: &str = "generated";

/// The marker that declares a hand transcription.
pub const SHAPE_MARKER: &str = "@config-shape";
/// The marker that overrides one position the contract also describes.
pub const EXCEPT_MARKER: &str = "@config-shape-except";
/// The marker that keeps one position the contract describes not at all.
pub const NARROW_MARKER: &str = "@config-shape-narrow";

/// Keywords in the order a generated block spells them: what the value *is*, then its bounds, then
/// what it holds.
///
/// Anything outside this list is appended alphabetically rather than dropped — under-reporting a
/// constraint is the failure this whole toolchain exists to remove, and it would be no better
/// coming from a renderer.
const ORDER: [&str; 19] = [
    "type",
    "enum",
    "const",
    "pattern",
    "format",
    "minimum",
    "exclusiveMinimum",
    "maximum",
    "exclusiveMaximum",
    "multipleOf",
    "minLength",
    "maxLength",
    "minItems",
    "maxItems",
    "uniqueItems",
    "required",
    "properties",
    "additionalProperties",
    "items",
];

/// Whether a binding class has a constraint of its own to derive a block from.
///
/// `composed` is the one left out, and the reason is that such a value is *an input* to the key's
/// text rather than the key's value — a port inside a `printf` — so the key's constraint describes
/// the composition and generating from it would type the part as the whole.
fn derives(class: Class) -> bool {
    matches!(
        class,
        Class::Projection | Class::Structured | Class::External
    )
}

/// One value whose block is owned here, generated or transcribed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    /// The chart.
    pub chart: String,
    /// The line a reader has to go to: a declared shape's marker, or a generated one's opening
    /// delimiter.
    pub line: usize,
    /// The value.
    pub values_path: String,
    /// Whether the chart owns the block rather than deriving it.
    pub handwritten: bool,
    /// The release the struct was read at, for a transcription.
    pub version: Option<String>,
    /// The file it was read from.
    pub source: Option<String>,
    /// Whether a marker declared this. A generated shape cannot be told to move a marker it does
    /// not have.
    pub declared: bool,
}

impl Shape {
    /// `chart/values.yaml:LINE`, for a message a reader can jump to.
    pub fn at(&self) -> String {
        format!("{}/values.yaml:{}", self.chart, self.line)
    }
}

/// Which of the two things a declared departure is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Departure {
    /// Replaces a position the contract also describes.
    Override,
    /// Keeps a position the contract does not describe at all.
    Narrowing,
}

impl Departure {
    /// The marker this was written as.
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Override => EXCEPT_MARKER,
            Self::Narrowing => NARROW_MARKER,
        }
    }
}

/// One declared departure from the generated shape: a position the chart keeps as its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    /// The chart.
    pub chart: String,
    /// The marker's own line.
    pub line: usize,
    /// The value whose block it departs from.
    pub values_path: String,
    /// A dotted walk of *schema keywords* — `items.properties.guild_id` — because that is what the
    /// thing being addressed is.
    pub sub_path: String,
    /// Why. Mandatory: a departure from the contract without a reason is one nobody can review.
    pub reason: String,
    /// Which kind.
    pub kind: Departure,
}

impl Divergence {
    /// `chart/values.yaml:LINE`, for a message a reader can jump to.
    pub fn at(&self) -> String {
        format!("{}/values.yaml:{}", self.chart, self.line)
    }
}

/// One image publishing a key, and the release its vendored contract was published at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publisher {
    /// The document that carries the key.
    pub document: String,
    /// The release its image was built at.
    pub version: String,
}

/// The contract key one value feeds, and everything a caller needs about it.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// What the producer says the parsed value must satisfy.
    pub constraint: Option<Json>,
    /// Whether the marker called the binding optional.
    pub optional: bool,
    /// Whether every document that carries it calls it `structured`.
    pub structured: bool,
    /// The producer's own name for the type, carried for one purpose: a refusal that asks for a
    /// transcription can name what is being transcribed, so the marker it asks for can be pasted
    /// rather than composed. Empty when the documents disagree, or publish none.
    pub ty: String,
    /// Every document that carries the key, paired with the release its image was built at.
    pub publishers: Vec<Publisher>,
}

impl Resolved {
    /// The furthest-ahead release publishing this key — what a transcription is read at.
    ///
    /// A hand copy is one description of a type several images share, so the release to read it at
    /// is the newest of them: read at the newest it is a true statement about that image and at
    /// worst a narrowing of an older one, whereas read at the oldest it says nothing about the
    /// field the newest added.
    pub fn newest(&self) -> &str {
        self.publishers
            .iter()
            .max_by_key(|held| ordinal(&held.version))
            .map_or("", |held| held.version.as_str())
    }
}

/// A release as a comparable tuple, for the one comparison made here.
///
/// Leading numeric segments only: `8.9.1` sorts under `8.10.0`, which a string comparison gets
/// backwards, and a tag nobody writes — a pre-release, a date, a bare word — degrades to nothing
/// rather than failing. Two of those compare equal, which the caller turns into "re-read it" rather
/// than "it is current".
fn ordinal(version: &str) -> Vec<u64> {
    let mut parts = Vec::new();
    for segment in super::release(version).split('.') {
        match segment.parse::<u64>() {
            Ok(number) => parts.push(number),
            Err(_) => break,
        }
    }
    parts
}

// ------------------------------------------------------------------------------------------
// Reading the markers
// ------------------------------------------------------------------------------------------

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
fn read_divergence(
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
fn marker_body<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let rest = line.trim_start().strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix(marker)?;
    let body = rest.strip_prefix(|held: char| held.is_whitespace())?.trim();
    (!body.is_empty()).then_some(body)
}

// ------------------------------------------------------------------------------------------
// Reading a block
// ------------------------------------------------------------------------------------------

/// One block's lines, as `(the marker run, the schema)`.
///
/// The marker run is the `# # @config ...` lines the block opens with — hand-written, and none of
/// this module's business. Everything after it is the schema, and is what a regeneration replaces.
pub fn split_block(block: &Block) -> (Vec<String>, Vec<String>) {
    let mut markers = Vec::new();
    let mut rest = Vec::new();
    let mut in_run = true;
    for line in &block.lines {
        if in_run && super::markers::opens_with_a_marker(line) {
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

// ------------------------------------------------------------------------------------------
// Building the expected schema
// ------------------------------------------------------------------------------------------

/// Whether a constraint says anything a schema could hold a value to.
///
/// A key the producer publishes no constraint for would leave [`expected`] with a block naming all
/// six JSON types, which is a property a schema generator emits and nothing rejects. As an opt-in
/// that was harmless; as the default it is a downgrade, so the generator refuses the key instead and
/// asks for the transcription to be declared as one.
pub fn describing(constraint: Option<&Json>) -> bool {
    constraint.and_then(Json::as_object).is_some_and(|held| {
        ["type", "enum", "const"]
            .iter()
            .any(|name| held.contains_key(*name))
    })
}

/// What one element of a container-typed key holds, or [`None`] when the contract does not say.
///
/// `items` for a sequence and `additionalProperties` for a map, which is the whole of the
/// `schema_version: 2` addition at one level. `true` and `false` are the open/closed flag rather
/// than a schema, and are not an element description.
pub fn element_schema(constraint: Option<&Json>) -> Option<&Json> {
    let held = constraint?.as_object()?;
    ["items", "additionalProperties"]
        .into_iter()
        .find_map(|keyword| {
            held.get(keyword)
                .filter(|element| element.as_object().is_some_and(|held| !held.is_empty()))
        })
}

/// Whether the contract says what one element of a container-typed key holds.
pub fn describes_element(constraint: Option<&Json>) -> bool {
    element_schema(constraint).is_some()
}

/// The `@schema` mapping one contract key's constraint calls for.
///
/// Two shapes are not a straight copy, and both were measured against the schema generator rather
/// than reasoned about:
///
/// **An `enum` is the whole block.** The generator refuses one carrying both `enum` and `type`, and
/// exits fatally, leaving every chart in the repository without a generated schema. So a key whose
/// constraint names an `enum` emits its members alone, with `null` joining them where the value is
/// optional. Measured again for `schema_version: 2`: the refusal applies to the *top level only*. A
/// nested subschema carrying both passes through, and that is what makes a generated element schema
/// possible at all — every enum inside a struct is an `enum` beside a `type`.
///
/// **A `structured` key whose element is undescribed is opened rather than described.** Such a
/// constraint says the value is a table and nothing about what is in it, so the block accepts any
/// table. A key whose element *is* described takes the description instead, which is the whole of
/// what `schema_version: 2` buys.
///
/// **A `structured` key whose constraint names an array is neither.** It carries its own `items`,
/// and the value beside it is a list, so describing it as an object makes a chart reject its own
/// defaults the moment its schema is generated.
pub fn expected(constraint: Option<&Json>, optional: bool, structured: bool) -> Json {
    let empty = Map::new();
    let held = constraint.and_then(Json::as_object).unwrap_or(&empty);

    if let Some(Json::Array(members)) = held.get("enum") {
        let mut members = members.clone();
        if optional && !members.contains(&Json::Null) {
            members.push(Json::Null);
        }
        let mut schema = Map::new();
        schema.insert("enum".to_owned(), Json::Array(members));
        return Json::Object(schema);
    }

    let declared = held.get("type");
    let mut types: Vec<String> = match declared {
        Some(Json::Array(names)) => names
            .iter()
            .filter(|name| !name.is_null())
            .map(scalar_name)
            .collect(),
        Some(Json::Null) | None => Vec::new(),
        Some(single) => vec![scalar_name(single)],
    };

    let mut schema = Map::new();
    let element = element_schema(constraint);

    if structured
        && (types.is_empty() || types.iter().any(|name| name == "object"))
        && !held.contains_key("items")
    {
        types = vec!["object".to_owned()];
        // The producer's own fields, where it published any. A table key is normally split into one
        // contract key per field, so this is the hand-written case rather than the derive's — and
        // dropping the fields there would describe a documented struct as an open table.
        for keyword in ["required", "properties"] {
            if let Some(value) = held.get(keyword) {
                schema.insert(keyword.to_owned(), copy_keyword(keyword, value));
            }
        }
        // `additionalProperties: true` is the open flag, and an element schema replaces it: a map
        // whose values are all one shape is not open, it is uniform. Written even beside enumerated
        // properties, because a generator injects `additionalProperties: false` into a top-level
        // block that enumerates them and says nothing — and the contract's silence here means open,
        // not closed: a deserialiser accepts a field nobody declared unless the type says otherwise.
        schema.insert(
            "additionalProperties".to_owned(),
            element.map_or(Json::Bool(true), copy),
        );
    } else {
        for keyword in ORDER {
            if keyword == "type" || keyword == "enum" {
                continue;
            }
            if let Some(value) = held.get(keyword) {
                schema.insert(keyword.to_owned(), copy_keyword(keyword, value));
            }
        }
    }

    if types.is_empty() {
        // The constraint names no type. Rather than guess one, accept every JSON type — the
        // contract is still the authority on the value, and the document gate holds the rendered
        // configuration against it either way.
        types = ["string", "integer", "boolean", "array", "object", "null"]
            .into_iter()
            .map(str::to_owned)
            .collect();
    }
    // An optional value is one the chart may legitimately leave unset, and a derived helper omits it
    // rather than writing it empty — so `null` has to be a value the schema accepts.
    if optional && !types.iter().any(|name| name == "null") {
        types.push("null".to_owned());
    }

    let mut out = Map::new();
    out.insert(
        "type".to_owned(),
        if types.len() == 1 {
            Json::String(types.remove(0))
        } else {
            Json::Array(types.into_iter().map(Json::String).collect())
        },
    );
    for (name, value) in schema {
        out.insert(name, value);
    }
    Json::Object(out)
}

/// One JSON scalar as the name a `type` keyword spells it with.
fn scalar_name(value: &Json) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// One subschema, with every annotation dropped at every level.
///
/// `format` is kept: it is the one annotation a validator acts on. Everything else is copied rather
/// than translated — a constraint is JSON Schema and so is an `@schema` block, so a `minimum` means
/// the same thing on both sides.
fn copy(value: &Json) -> Json {
    match value {
        Json::Object(fields) => Json::Object(
            fields
                .iter()
                .filter(|(name, _)| !is_annotation(name))
                .map(|(name, inner)| (name.clone(), copy_keyword(name, inner)))
                .collect(),
        ),
        Json::Array(items) => Json::Array(items.iter().map(copy).collect()),
        other => other.clone(),
    }
}

/// One keyword's value, copied as the thing that keyword holds.
///
/// `properties` is the only container keyword whose value is a mapping of *names* to schemas;
/// `items` and `additionalProperties` each hold a schema outright. So it is the one place a key must
/// not be read as a keyword, and the one place [`copy`] may not strip by name — a struct with a
/// field called `description` is an ordinary struct.
fn copy_keyword(keyword: &str, value: &Json) -> Json {
    if keyword == "properties"
        && let Json::Object(fields) = value
    {
        return Json::Object(
            fields
                .iter()
                .map(|(name, subschema)| (name.clone(), copy(subschema)))
                .collect(),
        );
    }
    copy(value)
}

/// Whether a keyword says something to a reader and asserts nothing about a value.
fn is_annotation(name: &str) -> bool {
    matches!(
        name,
        "description" | "title" | "default" | "examples" | "$comment"
    )
}

// ------------------------------------------------------------------------------------------
// Declared divergences
// ------------------------------------------------------------------------------------------

/// The generated schema with each declared position taken from the chart's own block.
///
/// Returns the result and the problems found, rather than stopping at the first: a value with two
/// stale departures should report both.
///
/// A sub-path always has to resolve in the chart's own block, since that is where the kept text
/// comes from. Whether it resolves in the *generated* schema is what the two markers disagree about,
/// and holding each to its own answer is what makes a departure age visibly:
///
/// | Marker | In the generated shape | What it says |
/// |---|---|---|
/// | `@config-shape-except` | present | the contract describes this, and the chart overrides it |
/// | `@config-shape-narrow` | absent | the contract describes nothing here, and the chart adds a bound |
///
/// Written the wrong way round, each is refused with the other named. That is not pedantry: a
/// narrowing whose position the contract *starts* describing is exactly the moment somebody has to
/// decide whether the producer's own bound supersedes the chart's, and an override whose position
/// the contract *stops* describing is a keyword nothing upstream stands behind any more. Both are
/// silent under a single marker that accepts either.
pub fn apply_divergences(
    generated: &Json,
    present: &Json,
    divergences: &[&Divergence],
) -> (Json, Added, Vec<String>) {
    let mut result = generated.clone();
    let mut added = Added::default();
    let mut problems = Vec::new();

    for divergence in divergences {
        let parts: Vec<&str> = divergence.sub_path.split('.').collect();
        let Some(kept) = dig(present, &parts).cloned() else {
            problems.push(format!(
                "{}: keeps '{}' of '{}', which the block below does not declare — there is \
                 nothing to keep",
                divergence.at(),
                divergence.sub_path,
                divergence.values_path
            ));
            continue;
        };

        let described = dig(generated, &parts).is_some();
        if divergence.kind == Departure::Override && !described {
            problems.push(format!(
                "{}: overrides '{}' of '{}', which the generated schema does not contain. Either \
                 the contract no longer describes it — in which case this is a narrowing and the \
                 marker is `{NARROW_MARKER}` — or the sub-path is a typo",
                divergence.at(),
                divergence.sub_path,
                divergence.values_path
            ));
            continue;
        }
        if divergence.kind == Departure::Narrowing && described {
            problems.push(format!(
                "{}: narrows '{}' of '{}', which the contract now describes itself. Decide \
                 between them: `{EXCEPT_MARKER}` keeps the chart's, and deleting the marker takes \
                 the image's",
                divergence.at(),
                divergence.sub_path,
                divergence.values_path
            ));
            continue;
        }
        if !described && dig(&result, &parts[..parts.len() - 1]).is_none() {
            problems.push(format!(
                "{}: narrows '{}' of '{}', whose enclosing position the generated schema does not \
                 contain either, so there is nowhere to put it",
                divergence.at(),
                divergence.sub_path,
                divergence.values_path
            ));
            continue;
        }

        if !described {
            added.record(&parts);
        }
        put(&mut result, &parts, kept);
    }

    (settle(result), added, problems)
}

/// The struct fields a narrowing put into the generated schema, by the position they sit at.
///
/// A document's member order is a fact the reader here cannot keep — every object is read through a
/// sorted map, which is what makes a re-rendered document byte-identical to the one a producer
/// wrote — so a generated block spells the producer's fields in the order the document gives them
/// back. A field the *chart* adds has no such order to recover, and appending it is the only answer
/// that is stable: sorting it in would move it the first time somebody renamed it, and every value
/// in the chart would come out with a different diff.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Added {
    positions: Vec<String>,
}

impl Added {
    /// Remember one position, if it names a struct field rather than a keyword.
    fn record(&mut self, parts: &[&str]) {
        if parts.len() >= 2 && parts[parts.len() - 2] == "properties" {
            self.positions.push(parts.join("."));
        }
    }

    /// Whether the field at this position was added rather than published.
    fn holds(&self, at: &str, field: &str) -> bool {
        let full = if at.is_empty() {
            format!("properties.{field}")
        } else {
            format!("{at}.properties.{field}")
        };
        self.positions.contains(&full)
    }
}

/// One assembled block, with the pair a schema generator refuses resolved in the enum's favour.
///
/// [`expected`] never emits `enum` beside `type` at the top level, because a generator exits fatally
/// on it and leaves every chart without a schema. A narrowing can reintroduce the pair — a chart
/// that enumerates the members of a key the contract only types as a string is the ordinary case —
/// so the same rule is applied after the departures rather than only before. The enum wins because
/// it is the stricter of the two and because it is the half somebody wrote down deliberately.
fn settle(schema: Json) -> Json {
    let Json::Object(mut fields) = schema else {
        return schema;
    };
    if fields.contains_key("enum") && fields.contains_key("type") {
        fields.remove("type");
    }
    Json::Object(fields)
}

/// One position inside a schema, by a dotted walk of keywords.
fn dig<'a>(schema: &'a Json, parts: &[&str]) -> Option<&'a Json> {
    let mut current = schema;
    for part in parts {
        current = current.as_object()?.get(*part)?;
    }
    Some(current)
}

/// Write one position inside a schema, whose enclosing position the caller has established.
fn put(schema: &mut Json, parts: &[&str], value: Json) {
    let mut current = schema;
    for part in &parts[..parts.len() - 1] {
        let Some(next) = current.as_object_mut().and_then(|held| held.get_mut(*part)) else {
            return;
        };
        current = next;
    }
    if let Some(held) = current.as_object_mut() {
        held.insert(
            (*parts.last().expect("a sub-path has a last part")).to_owned(),
            value,
        );
    }
}

// ------------------------------------------------------------------------------------------
// Writing it back out
// ------------------------------------------------------------------------------------------

/// One `@schema` mapping as the comment lines a values file carries it as.
pub fn render(schema: &Json, indent: usize, added: &Added) -> Vec<String> {
    let padding = " ".repeat(indent);
    lines_of(schema, "", added)
        .into_iter()
        .map(|line| {
            if line.is_empty() {
                format!("{padding}#")
            } else {
                format!("{padding}# {line}")
            }
        })
        .collect()
}

/// The schema as YAML, in [`ORDER`], block form throughout.
///
/// Block form rather than flow, because that is what a hand-written block uses and these lines are
/// read by people first. Written here rather than handed to a YAML emitter, which would sort keys
/// alphabetically, wrap mid-value and quote strings on rules of its own — three things that would
/// each show up as churn in a generated file.
fn lines_of(schema: &Json, at: &str, added: &Added) -> Vec<String> {
    let Some(fields) = schema.as_object() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for name in ordered(fields) {
        let below = if at.is_empty() {
            name.clone()
        } else {
            format!("{at}.{name}")
        };
        lines.extend(keyword_lines(&name, &fields[&name], &below, at, added));
    }
    lines
}

/// The keywords of one level, canonical ones first and the rest alphabetically after.
fn ordered(fields: &Map<String, Json>) -> Vec<String> {
    let mut known: Vec<String> = ORDER
        .iter()
        .filter(|name| fields.contains_key(**name))
        .map(|name| (*name).to_owned())
        .collect();
    let mut rest: Vec<String> = fields
        .keys()
        .filter(|name| !ORDER.contains(&name.as_str()))
        .cloned()
        .collect();
    rest.sort();
    known.extend(rest);
    known
}

/// One JSON Schema keyword as the YAML lines an `@schema` comment carries.
fn keyword_lines(name: &str, value: &Json, below: &str, at: &str, added: &Added) -> Vec<String> {
    if name == "type" {
        return vec![type_line(value)];
    }
    if name == "properties"
        && let Some(fields) = value.as_object().filter(|fields| !fields.is_empty())
    {
        // Field names, not keywords. Kept in the order the producer declared them — that is the
        // order the type is written in and the order its documentation reads in, and sorting
        // them would make a twenty-field element unrecognisable against the source it came from.
        let mut lines = vec!["properties:".to_owned()];
        let mut names: Vec<&String> = fields.keys().collect();
        // The producer's fields in the order the document gives them back, then whatever a
        // narrowing added, in the order the markers were written.
        names.sort_by_key(|field| usize::from(added.holds(at, field)));
        for field in names {
            let subschema = &fields[field];
            // Rendered here rather than through this function, which reads its first argument
            // as a schema keyword: a struct with a field called `type` or `items` would
            // otherwise be spelt as the keyword of that name and come out as something else.
            match subschema.as_object().filter(|held| !held.is_empty()) {
                Some(_) => {
                    lines.push(format!("  {}:", field_name(field)));
                    lines.extend(
                        lines_of(subschema, &format!("{below}.{field}"), added)
                            .into_iter()
                            .map(|line| format!("    {line}")),
                    );
                }
                None => lines.push(format!("  {}: {{}}", field_name(field))),
            }
        }
        return lines;
    }
    match value {
        Json::Object(fields) if fields.is_empty() => vec![format!("{name}: {{}}")],
        Json::Object(_) => {
            let mut lines = vec![format!("{name}:")];
            lines.extend(
                lines_of(value, below, added)
                    .into_iter()
                    .map(|line| format!("  {line}")),
            );
            lines
        }
        Json::Array(items) if items.iter().any(|item| item.is_object() || item.is_array()) => {
            let mut lines = vec![format!("{name}:")];
            for item in items {
                if item.is_object() {
                    let rendered = lines_of(item, below, added);
                    let mut held = rendered.into_iter();
                    if let Some(first) = held.next() {
                        lines.push(format!("  - {first}"));
                        lines.extend(held.map(|line| format!("    {line}")));
                    }
                } else {
                    lines.push(format!("  - {}", scalar(item)));
                }
            }
            lines
        }
        other => vec![format!("{name}: {}", scalar(other))],
    }
}

/// Field names YAML 1.1 reads as something other than a string.
///
/// `yes`, `no`, `on` and `off` are the ones that bite — a struct field called `on` is an ordinary
/// field name in every language a producer is written in, and unquoted it is the boolean true.
const YAML_WORDS: [&str; 11] = [
    "y", "n", "yes", "no", "on", "off", "true", "false", "null", "none", "~",
];

/// One struct field name, as the mapping key an `@schema` block carries it as.
fn field_name(name: &str) -> String {
    let plain = !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|held| held.is_ascii_alphabetic() || held == '_')
        && name
            .chars()
            .all(|held| held.is_ascii_alphanumeric() || matches!(held, '_' | '.' | '-'));
    if plain && !YAML_WORDS.contains(&name.to_lowercase().as_str()) {
        name.to_owned()
    } else {
        quoted(name)
    }
}

/// The `type` keyword, whose members are schema vocabulary rather than data.
///
/// Written bare for that reason. `null` is the one member that has to be quoted: unquoted it is
/// YAML's null, and the schema would then declare no type at all where it meant the null type.
fn type_line(value: &Json) -> String {
    let names: Vec<String> = match value {
        Json::Array(items) => items.iter().map(scalar_name).collect(),
        single => vec![scalar_name(single)],
    };
    let rendered: Vec<String> = names
        .iter()
        .map(|name| {
            if name == "null" {
                "'null'".to_owned()
            } else {
                name.clone()
            }
        })
        .collect();
    if names.len() == 1 {
        format!("type: {}", rendered[0])
    } else {
        format!("type: [{}]", rendered.join(", "))
    }
}

/// One keyword's value, as the YAML an `@schema` comment carries.
///
/// `null` inside an `enum` is the null value and is written bare, which is the spelling a
/// hand-written block already uses; the `type` keyword is where it has to be quoted, and that is
/// [`type_line`]'s business.
fn scalar(value: &Json) -> String {
    match value {
        Json::Bool(true) => "true".to_owned(),
        Json::Bool(false) => "false".to_owned(),
        Json::Null => "null".to_owned(),
        Json::Number(number) => number.to_string(),
        Json::Array(items) => format!(
            "[{}]",
            items.iter().map(scalar).collect::<Vec<_>>().join(", ")
        ),
        Json::String(text) => quoted(text),
        Json::Object(_) => "{}".to_owned(),
    }
}

/// A YAML double-quoted scalar. Used wherever a value could be read as something else.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

// ------------------------------------------------------------------------------------------
// Resolving one value against its contracts
// ------------------------------------------------------------------------------------------

/// The one binding a derived block can come from, or [`None`] with the refusal reported.
///
/// A value feeding several keys has no single constraint to be derived from, and a value feeding
/// none has nothing to derive from at all — so both are refusals rather than a choice.
fn one_binding<'a>(
    shape: &Shape,
    markers: &'a [Marker],
    refuse: &mut impl FnMut(String),
) -> Option<&'a Marker> {
    let derived: Vec<&Marker> = markers
        .iter()
        .filter(|marker| derives(marker.class))
        .collect();
    if derived.is_empty() {
        refuse(format!(
            "{}: '{}' carries no `{MARKER}` marker naming the contract key it feeds, so there is              nothing to derive its schema from",
            shape.at(),
            shape.values_path
        ));
        return None;
    }
    if derived.len() > 1 {
        refuse(format!(
            "{}: '{}' binds {}, and a value feeding several keys has no single constraint to be              derived from",
            shape.at(),
            shape.values_path,
            derived
                .iter()
                .map(|marker| crate::gate::quoted(&marker.target))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        return None;
    }
    Some(derived[0])
}

/// The contract key one value feeds, resolved against every document that carries it.
///
/// `Ok(None)` when the value cannot be resolved and the caller asked not to be told — a hand
/// transcription in a chart with no contract at all is the ordinary state of one, not a defect.
pub fn resolve(
    bound: Option<&Bound>,
    shape: &Shape,
    markers: &[Marker],
    problems: &mut Vec<String>,
    report: bool,
) -> Option<Resolved> {
    let mut refuse = |message: String| {
        if report {
            problems.push(message);
        }
    };

    let marker = one_binding(shape, markers, &mut refuse)?;
    let Some(bound) = bound else {
        refuse(format!(
            "{}: '{}' binds '{}', and this chart has no {} naming the document that declares it",
            shape.at(),
            shape.values_path,
            marker.target,
            super::DECLARATION
        ));
        return None;
    };

    let candidates: Vec<String> = match &marker.documents {
        Some(scope) => scope.clone(),
        None => bound.documents.keys().cloned().collect(),
    };
    // Kept paired with the document that carried it rather than flattened: which images publish the
    // key is what decides the release a hand transcription is held against, and for a multi-service
    // chart that is a strict subset of the documents it declares.
    let carried: Vec<(&String, &Merged)> = candidates
        .iter()
        .filter_map(|name| {
            bound
                .namespace(name, marker.class.targets_a_key())
                .and_then(|namespace| namespace.get(&marker.target))
                .map(|entry| (name, entry))
        })
        .collect();
    if carried.is_empty() {
        // The bindings gate reports this one properly, with the suggestion and the scope
        // diagnostic. Repeating that here would print the same defect twice.
        refuse(format!(
            "{}: '{}' binds '{}', which no contract this chart declares carries — see the \
             bindings gate",
            shape.at(),
            shape.values_path,
            marker.target
        ));
        return None;
    }

    let first = carried[0].1.fields.get("constraint");
    if carried
        .iter()
        .any(|(_, entry)| entry.fields.get("constraint") != first)
    {
        refuse(format!(
            "{}: the documents this chart declares describe '{}' differently, so no one schema is \
             derivable from them",
            shape.at(),
            marker.target
        ));
        return None;
    }

    let structured = carried
        .iter()
        .all(|(_, entry)| entry.text("text_form") == Some("structured"));
    let names: std::collections::BTreeSet<&str> = carried
        .iter()
        .map(|(_, entry)| entry.text("ty").unwrap_or(""))
        .collect();
    let publishers: Vec<Publisher> = carried
        .iter()
        .flat_map(|(name, _)| {
            bound
                .releases
                .get(*name)
                .into_iter()
                .flatten()
                .map(move |version| Publisher {
                    document: (*name).clone(),
                    version: version.clone(),
                })
        })
        .collect();

    Some(Resolved {
        constraint: first.filter(|held| !held.is_null()).cloned(),
        optional: marker.optional,
        structured,
        ty: if names.len() == 1 {
            names.into_iter().next().unwrap_or("").to_owned()
        } else {
            String::new()
        },
        publishers,
    })
}

/// Whether a block read at one release still describes an image published at another.
///
/// True when the two are the same release, and when the first is the later: a copy read at 8.9.1
/// covers a service still at 8.9.0, at worst by narrowing it, and demanding a re-read there would
/// fail a chart the moment its images stopped moving in lockstep.
///
/// Ordering is consulted only when both parse as a release. A tag nobody writes degrades to nothing,
/// and two of those would compare equal and pass; such a pair is held to equality instead, which
/// errs towards asking for a re-read rather than towards a stale copy that looks maintained.
pub fn covers(read_at: &str, published: &str) -> bool {
    if super::release(read_at) == super::release(published) {
        return true;
    }
    let (mine, theirs) = (ordinal(read_at), ordinal(published));
    !mine.is_empty() && !theirs.is_empty() && mine >= theirs
}

/// Whether a hand-transcribed shape is one the contract now publishes itself.
///
/// Two ways for that to become true, and they are the two reasons a transcription is allowed in the
/// first place: a container whose element the producer had not described, and a key it published no
/// constraint for at all. Either one arriving retires the hand copy.
pub fn superseded(resolved: &Resolved) -> bool {
    if resolved.structured {
        describes_element(resolved.constraint.as_ref())
    } else {
        describing(resolved.constraint.as_ref())
    }
}

/// Every generated shape a chart has, from its markers and its blocks.
///
/// Which is every bound value that keeps no transcription marker. Enrolment used to be per value
/// and opt-in, and the default is now the other way round, so a key an image retypes moves the chart
/// on the next run whether or not anybody remembered to enrol the value.
pub fn generated(
    chart: &str,
    markers: &BTreeMap<String, Vec<Marker>>,
    blocks: &BTreeMap<String, Block>,
    transcribed: &[Shape],
) -> Vec<Shape> {
    let declared: Vec<&str> = transcribed
        .iter()
        .map(|shape| shape.values_path.as_str())
        .collect();

    let mut shapes: Vec<Shape> = markers
        .iter()
        .filter(|(values_path, held)| {
            !declared.contains(&values_path.as_str())
                && held.iter().any(|marker| derives(marker.class))
        })
        .map(|(values_path, held)| Shape {
            chart: chart.to_owned(),
            line: blocks
                .get(values_path)
                .map_or_else(|| held[0].line, |block| block.start),
            values_path: values_path.clone(),
            handwritten: false,
            version: None,
            source: None,
            declared: false,
        })
        .collect();
    shapes.sort_by_key(|shape| shape.line);
    shapes
}

// ------------------------------------------------------------------------------------------
// The walk over one chart
// ------------------------------------------------------------------------------------------

/// One chart's shape markers, resolved against its contracts.
///
/// Everything that can fail before a single block is looked at fails here, with the chart named
/// once: a marker naming a value that does not exist, a departure with no shape to depart from, a
/// value that binds no key. The two callers then differ only in what they do with a block that does
/// not match.
pub struct Chart {
    /// The chart's directory name.
    pub name: String,
    /// The values file, verbatim. Read without newline translation, because a rewrite that
    /// normalised line endings would reflow every line of every file it touched.
    pub text: String,
    /// Everything found wrong, in the order it was found.
    pub problems: Vec<String>,
    /// The values whose block the chart owns.
    pub transcribed: Vec<Shape>,
    /// The declared departures from a generated block.
    pub divergences: Vec<Divergence>,
    /// Every `@schema` block, by the value it belongs to.
    pub blocks: BTreeMap<String, Block>,
    /// Every binding marker, by the value it was written in.
    pub markers: BTreeMap<String, Vec<Marker>>,
    /// The chart's documents, resolved to their contracts.
    pub bound: Option<Bound>,
    /// The chart's own `appVersion`, for a transcription no contract carries.
    pub app_version: String,
    /// Every shape: the generated ones, then the transcribed.
    pub shapes: Vec<Shape>,
}

impl Chart {
    /// Read one chart, resolving everything that can be resolved before a block is compared.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the markers or the declaration cannot be read at all, and
    /// [`Error::Io`] when the values file cannot be.
    pub fn read(chart_dir: &std::path::Path) -> Result<Self, Error> {
        let name = chart_dir
            .file_name()
            .map_or_else(String::new, |held| held.to_string_lossy().into_owned());
        let values = chart_dir.join("values.yaml");
        let bytes = std::fs::read(&values).map_err(|e| Error::io(values.display(), e))?;
        let text = String::from_utf8(bytes)
            .map_err(|e| Error::Invalid(format!("{}: is not UTF-8: {e}", values.display())))?;

        let (transcribed, divergences) = parse_markers(&text, &name)?;
        let (found, blocks) = super::markers::read(&text, &name)?;

        let mut by_value: BTreeMap<String, Vec<Marker>> = BTreeMap::new();
        for marker in found {
            by_value
                .entry(marker.values_path.clone())
                .or_default()
                .push(marker);
        }
        let blocks: BTreeMap<String, Block> = blocks
            .into_iter()
            .map(|block| (block.values_path.clone(), block))
            .collect();

        let declaration = super::load_declaration(chart_dir)?;
        let bound = match declaration {
            Some(declaration) => Some(Bound::of(chart_dir, declaration)?),
            None => None,
        };

        let chart_yaml = chart_dir.join("Chart.yaml");
        let app_version = if chart_yaml.is_file() {
            let held = std::fs::read_to_string(&chart_yaml)
                .map_err(|e| Error::io(chart_yaml.display(), e))?;
            super::declaration::read_yaml(&held, &chart_yaml)?
                .get("appVersion")
                .map(scalar_name)
                .unwrap_or_default()
                .trim()
                .to_owned()
        } else {
            String::new()
        };

        let mut shapes = generated(&name, &by_value, &blocks, &transcribed);
        shapes.extend(transcribed.iter().cloned());

        let mut chart = Self {
            name,
            text,
            problems: Vec::new(),
            transcribed,
            divergences,
            blocks,
            markers: by_value,
            bound,
            app_version,
            shapes,
        };
        chart.check_markers();
        Ok(chart)
    }

    /// What can be decided from the markers alone, before a contract is opened.
    ///
    /// A value carries at most one shape marker, and a departure belongs to a generated block and
    /// says so by naming the same value.
    fn check_markers(&mut self) {
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        let mut problems = Vec::new();
        for shape in &self.transcribed {
            if let Some(first) = seen.get(&shape.values_path) {
                problems.push(format!(
                    "{}: '{}' already declares a shape on line {first}. One value has one block, \
                     so the second marker is either a duplicate or the migration of the first left \
                     half-done",
                    shape.at(),
                    shape.values_path
                ));
                continue;
            }
            seen.insert(shape.values_path.clone(), shape.line);
        }

        let derived: Vec<&str> = self
            .shapes
            .iter()
            .filter(|shape| !shape.handwritten)
            .map(|shape| shape.values_path.as_str())
            .collect();
        for divergence in &self.divergences {
            if derived.contains(&divergence.values_path.as_str()) {
                continue;
            }
            if seen.contains_key(&divergence.values_path) {
                problems.push(format!(
                    "{}: departs from the generated shape of '{}', whose block is `{HANDWRITTEN}` \
                     and so is not generated at all. A departure from a hand-written block is just \
                     the block",
                    divergence.at(),
                    divergence.values_path
                ));
                continue;
            }
            problems.push(format!(
                "{}: departs from the generated shape of '{}', which carries no `{MARKER}` marker \
                 naming a contract key, so nothing is generated there to depart from",
                divergence.at(),
                divergence.values_path
            ));
        }
        self.problems.extend(problems);
    }

    /// The contract key one value feeds.
    pub fn constraint_for(&mut self, shape: &Shape, report: bool) -> Option<Resolved> {
        let markers = self
            .markers
            .get(&shape.values_path)
            .cloned()
            .unwrap_or_default();
        let mut problems = Vec::new();
        let resolved = resolve(self.bound.as_ref(), shape, &markers, &mut problems, report);
        self.problems.extend(problems);
        resolved
    }

    /// The `@schema` block one shape describes, refusing a marker that is not above it.
    ///
    /// Placement is checked rather than assumed, for a shape a marker declares. The marker is
    /// matched to its value by name, so one written at the other end of the file would still
    /// resolve — and would then assert something about a block nobody reading that block can see.
    ///
    /// A generated shape is not declared anywhere, so there is nothing to place: it *is* the block,
    /// found through the `@config` marker written inside it.
    pub fn block_for(&mut self, shape: &Shape) -> Option<Block> {
        let Some(block) = self.blocks.get(&shape.values_path).cloned() else {
            self.problems.push(format!(
                "{}: names '{}', which has no `@schema` block for this marker to describe",
                shape.at(),
                shape.values_path
            ));
            return None;
        };
        if !shape.declared {
            return Some(block);
        }

        let lines: Vec<&str> = self.text.lines().collect();
        let between = lines
            .get(shape.line..block.start.saturating_sub(1))
            .unwrap_or(&[]);
        let interrupted = between
            .iter()
            .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
        if shape.line >= block.start || interrupted {
            self.problems.push(format!(
                "{}: sits away from the `@schema` block for '{}' on line {}. A marker is read next \
                 to the thing it describes or it is not read at all; write it in the comment run \
                 directly above the block",
                shape.at(),
                shape.values_path,
                block.start
            ));
            return None;
        }
        Some(block)
    }

    /// The lines one generated block's schema half should carry, or [`None`] on a problem.
    pub fn target(&mut self, shape: &Shape) -> Option<Vec<String>> {
        let block = self.block_for(shape)?;
        let resolved = self.constraint_for(shape, true)?;

        let ty = resolved.ty.clone();
        let source = if !ty.is_empty() && !ty.contains([' ', ',']) {
            ty
        } else {
            "<source>".to_owned()
        };
        // The release the marker being asked for should record: the newest image publishing the
        // key, not the chart's `appVersion`, which in a multi-service chart is one service's.
        let version = if resolved.newest().is_empty() {
            if self.app_version.is_empty() {
                "<version>".to_owned()
            } else {
                self.app_version.clone()
            }
        } else {
            resolved.newest().to_owned()
        };

        if resolved.structured && !describes_element(resolved.constraint.as_ref()) {
            self.problems.push(format!(
                "{}: the contract describes '{}' as a container and says nothing about what one \
                 element holds, so a generated block would type-check nothing. Transcribe it and \
                 declare `{SHAPE_MARKER} {} {HANDWRITTEN} {version} {source}` until the image \
                 publishes the element",
                shape.at(),
                shape.values_path,
                shape.values_path
            ));
            return None;
        }
        if !describing(resolved.constraint.as_ref()) {
            self.problems.push(format!(
                "{}: the contract publishes no constraint for '{}', so a generated block would \
                 accept every JSON type and check nothing. Transcribe it and declare \
                 `{SHAPE_MARKER} {} {HANDWRITTEN} {version} {source}`, which is held against the \
                 version it was read at",
                shape.at(),
                shape.values_path,
                shape.values_path
            ));
            return None;
        }

        let wanted = expected(
            resolved.constraint.as_ref(),
            resolved.optional,
            resolved.structured,
        );
        let present = match block_schema(&block) {
            Ok(present) => present,
            Err(failure) => {
                self.problems.push(format!("{}: {failure}", shape.at()));
                return None;
            }
        };

        let mine: Vec<&Divergence> = self
            .divergences
            .iter()
            .filter(|divergence| divergence.values_path == shape.values_path)
            .collect();
        let (settled, added, problems) = apply_divergences(&wanted, &present, &mine);
        let failed = !problems.is_empty();
        self.problems.extend(problems);
        if failed {
            return None;
        }

        Some(render(&settled, block.indent, &added))
    }

    /// The older marker: a copy of a struct, held against the release it was copied at.
    ///
    /// Reached from the checker only. Every other problem here is one the writer repairs on the
    /// same pass, so raising it from both halves costs nothing; this one it cannot repair —
    /// re-reading a struct somebody else owns is a person's job, and a writer that refused over it
    /// would turn the job that repairs the tree into a gate over the one thing in it no repair
    /// reaches.
    ///
    /// The release is the newest *image publishing the key*, and only the chart's own when no
    /// contract publishes it at all. The difference is the whole of this check on a multi-service
    /// chart: an automated bump moves the one image that had a release, so holding every
    /// transcription against `appVersion` demands that a struct owned by the images that did not
    /// move be re-read at a release those images were never built at.
    pub fn check_handwritten(&mut self, shape: &Shape) {
        if self.block_for(shape).is_none() {
            return;
        }
        let resolved = self.constraint_for(shape, false);
        if resolved.as_ref().is_some_and(superseded) {
            self.problems.push(format!(
                "{}: '{}' was transcribed by hand, and the contract now describes it itself. \
                 Delete the marker and regenerate; declare any position the chart keeps with \
                 `{EXCEPT_MARKER}` or `{NARROW_MARKER}`",
                shape.at(),
                shape.values_path
            ));
            return;
        }

        let published = resolved
            .as_ref()
            .map(|held| held.newest().to_owned())
            .unwrap_or_default();
        if published.is_empty() && self.app_version.is_empty() {
            self.problems.push(format!(
                "{}: declares a shape for '{}', but no contract records the release the key was \
                 published at and the chart has no appVersion either, so there is nothing to hold \
                 the transcription against",
                shape.at(),
                shape.values_path
            ));
            return;
        }

        let read_at = shape.version.clone().unwrap_or_default();
        let source = shape.source.clone().unwrap_or_default();
        if published.is_empty() {
            // No contract carries the key — an unbound transcription, whose only release is the
            // chart's own. Nothing here can be more precise, and equality is the check.
            if super::release(&read_at) != super::release(&self.app_version) {
                self.problems.push(format!(
                    "{}: '{}' was transcribed at {read_at}, and the chart now pins appVersion {}\n    re-read {source} at {}, bring the `@schema` block for '{}' up to it, and move the marker",
                    shape.at(),
                    shape.values_path,
                    self.app_version,
                    self.app_version,
                    shape.values_path
                ));
            }
            return;
        }

        if covers(&read_at, &published) {
            return;
        }
        let mut ahead: Vec<&str> = resolved
            .as_ref()
            .map(|held| {
                held.publishers
                    .iter()
                    .filter(|one| !covers(&read_at, &one.version))
                    .map(|one| one.document.as_str())
                    .collect()
            })
            .unwrap_or_default();
        ahead.sort_unstable();
        ahead.dedup();
        self.problems.push(format!(
            "{}: '{}' was transcribed at {read_at}, and {} now publish{} it at {published}\n    re-read {source} at {published}, bring the `@schema` block for '{}' up to it, and move the marker",
            shape.at(),
            shape.values_path,
            ahead.join(", "),
            if ahead.len() == 1 { "es" } else { "" },
            shape.values_path
        ));
    }
}

/// The chart's values file with every generated block written from its contract.
///
/// Blocks are replaced from the bottom up so that an earlier replacement cannot move the line
/// numbers a later one was read at.
pub fn rewrite(chart: &mut Chart) -> (String, usize) {
    let shapes: Vec<Shape> = chart
        .shapes
        .iter()
        .filter(|shape| !shape.handwritten)
        .cloned()
        .collect();

    let mut pending: Vec<(Block, Vec<String>)> = Vec::new();
    for shape in &shapes {
        if let Some(target) = chart.target(shape)
            && let Some(block) = chart.blocks.get(&shape.values_path)
        {
            pending.push((block.clone(), target));
        }
    }
    pending.sort_by_key(|(block, _)| std::cmp::Reverse(block.start));

    let ending = if chart.text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = split_keeping_ends(&chart.text);
    let mut written = 0;

    for (block, target) in pending {
        let (markers, current) = split_block(&block);
        if current == target {
            continue;
        }
        let body: Vec<String> = markers
            .into_iter()
            .chain(target)
            .map(|line| format!("{line}{ending}"))
            .collect();
        lines.splice(block.start..block.end - 1, body);
        written += 1;
    }

    (lines.concat(), written)
}

/// Every generated block against what its contract calls for, reported onto the chart.
pub fn check(chart: &mut Chart) {
    let shapes: Vec<Shape> = chart
        .shapes
        .iter()
        .filter(|shape| !shape.handwritten)
        .cloned()
        .collect();

    for shape in &shapes {
        let Some(target) = chart.target(shape) else {
            continue;
        };
        let Some(block) = chart.blocks.get(&shape.values_path).cloned() else {
            continue;
        };
        let (_, current) = split_block(&block);
        if current == target {
            continue;
        }
        // Compared as text rather than as meaning, and the writer compares the same way. A block
        // that differs only in quoting is still a block a regeneration would rewrite, so a gate
        // that passed it would leave the writer with a diff to commit on a tree it had just called
        // current — which is how a generated artefact ends up perpetually dirty.
        chart.problems.push(format!(
            "{}: the `@schema` block for '{}' is not what its contract describes; regenerate it\n{}",
            shape.at(),
            shape.values_path,
            difference(&current, &target)
        ));
    }
}

/// One values file's lines, each keeping the ending it was written with.
fn split_keeping_ends(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(position) = rest.find('\n') {
        lines.push(rest[..=position].to_owned());
        rest = &rest[position + 1..];
    }
    if !rest.is_empty() {
        lines.push(rest.to_owned());
    }
    lines
}

/// Two blocks, side by side, for a message somebody has to act on.
///
/// Both in full rather than a minimal edit script: a generated block is a dozen lines, and showing
/// both is shorter to read than working out which of them moved.
fn difference(current: &[String], target: &[String]) -> String {
    let mut out = String::from("--- values.yaml\n");
    for line in current {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("--- the contract\n");
    for line in target {
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        Added, Departure, Divergence, apply_divergences, covers, expected, ordinal, parse_markers,
        render,
    };

    fn divergence(sub_path: &str, kind: Departure) -> Divergence {
        Divergence {
            chart: "x".to_owned(),
            line: 1,
            values_path: "a".to_owned(),
            sub_path: sub_path.to_owned(),
            reason: "because".to_owned(),
            kind,
        }
    }

    #[test]
    fn an_enum_is_the_whole_block() {
        // A generator refuses `enum` beside `type` and exits fatally, leaving every chart in the
        // repository without a schema.
        let held = expected(
            Some(&json!({"type": "string", "enum": ["a", "b"]})),
            false,
            false,
        );
        assert_eq!(held, json!({"enum": ["a", "b"]}));
    }

    #[test]
    fn an_optional_enum_gains_null_as_a_member() {
        let held = expected(Some(&json!({"enum": ["a"]})), true, false);
        assert_eq!(held, json!({"enum": ["a", null]}));
    }

    #[test]
    fn a_structured_key_with_no_element_is_opened_rather_than_described() {
        let held = expected(Some(&json!({"type": "object"})), false, true);
        assert_eq!(
            held,
            json!({"type": "object", "additionalProperties": true})
        );
    }

    #[test]
    fn a_structured_key_whose_element_is_described_takes_the_description() {
        // The whole of what `schema_version: 2` buys: the same field, carrying a schema rather
        // than `true`.
        let held = expected(
            Some(&json!({
                "type": "object",
                "additionalProperties": {"type": "object", "properties": {"token": {"type": "string"}}},
            })),
            false,
            true,
        );
        assert_eq!(
            held["additionalProperties"]["properties"]["token"],
            json!({"type": "string"})
        );
    }

    #[test]
    fn a_structured_key_whose_constraint_names_an_array_is_neither() {
        // Describing it as an object made a chart reject its own defaults the moment its schema
        // was generated.
        let held = expected(
            Some(&json!({"type": "array", "items": {"type": "string"}})),
            false,
            true,
        );
        assert_eq!(held["type"], json!("array"));
        assert_eq!(held["items"], json!({"type": "string"}));
    }

    #[test]
    fn a_constraint_with_no_type_accepts_every_json_type_rather_than_a_guess() {
        let held = expected(Some(&json!({"minimum": 1})), false, false);
        assert_eq!(
            held["type"],
            json!(["string", "integer", "boolean", "array", "object", "null"])
        );
    }

    #[test]
    fn an_optional_value_accepts_null() {
        let held = expected(Some(&json!({"type": "string"})), true, false);
        assert_eq!(held["type"], json!(["string", "null"]));
    }

    #[test]
    fn annotations_do_not_come_across_but_format_does() {
        let held = expected(
            Some(&json!({
                "type": "array",
                "items": {"type": "string", "description": "prose", "format": "uri"},
            })),
            false,
            false,
        );
        assert_eq!(held["items"], json!({"type": "string", "format": "uri"}));
    }

    #[test]
    fn a_struct_field_called_description_is_a_field_and_not_an_annotation() {
        let held = expected(
            Some(&json!({
                "type": "object",
                "properties": {"description": {"type": "string"}},
            })),
            false,
            true,
        );
        assert_eq!(held["properties"]["description"], json!({"type": "string"}));
    }

    #[test]
    fn a_release_is_ordered_numerically_rather_than_as_text() {
        assert!(ordinal("8.10.0") > ordinal("8.9.1"));
        assert!(covers("v8.9.1", "8.9.1"));
        assert!(covers("8.9.1", "8.9.0"));
        assert!(!covers("8.9.0", "8.9.1"));
        // A tag that is not a release is held to equality, which errs towards asking for a re-read.
        assert!(!covers("nightly", "8.9.1"));
        assert!(covers("nightly", "nightly"));
    }

    #[test]
    fn an_override_must_name_a_position_the_contract_describes() {
        let generated =
            json!({"type": "array", "items": {"properties": {"id": {"type": "integer"}}}});
        let present = json!({"type": "array", "items": {"properties": {"id": {"type": "string"}}}});
        let kept = divergence("items.properties.id", Departure::Override);
        let (result, _added, problems) = apply_divergences(&generated, &present, &[&kept]);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            result["items"]["properties"]["id"],
            json!({"type": "string"})
        );

        let wrong = divergence("items.properties.id", Departure::Narrowing);
        let (_, _added, problems) = apply_divergences(&generated, &present, &[&wrong]);
        assert!(
            problems[0].contains("the contract now describes itself"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_narrowing_must_name_a_position_the_contract_does_not_describe() {
        let generated = json!({"type": "object", "properties": {"a": {"type": "number"}}});
        let present = json!({
            "type": "object",
            "properties": {"a": {"type": "number"}, "b": {"type": "string"}},
        });
        let kept = divergence("properties.b", Departure::Narrowing);
        let (result, _added, problems) = apply_divergences(&generated, &present, &[&kept]);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(result["properties"]["b"], json!({"type": "string"}));

        let wrong = divergence("properties.a", Departure::Narrowing);
        let (_, _added, problems) = apply_divergences(&generated, &present, &[&wrong]);
        assert!(problems[0].contains("now describes itself"), "{problems:?}");
    }

    #[test]
    fn a_departure_naming_nothing_in_the_block_has_nothing_to_keep() {
        let kept = divergence("properties.nope", Departure::Narrowing);
        let (_, _added, problems) = apply_divergences(&json!({}), &json!({}), &[&kept]);
        assert!(problems[0].contains("nothing to keep"), "{problems:?}");
    }

    #[test]
    fn a_narrowing_that_reintroduces_an_enum_beside_a_type_settles_on_the_enum() {
        // A generator exits fatally on the pair, so the rule is applied after the departures as
        // well as before them.
        let generated = json!({"type": "string"});
        let present = json!({"type": "string", "enum": ["debug", "info"]});
        let kept = divergence("enum", Departure::Narrowing);
        let (result, _added, problems) = apply_divergences(&generated, &present, &[&kept]);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(result, json!({"enum": ["debug", "info"]}));
    }

    #[test]
    fn the_rendering_is_block_form_and_keeps_the_canonical_order() {
        let rendered = render(
            &json!({"minimum": 1, "type": "integer", "maximum": 9}),
            2,
            &Added::default(),
        );
        assert_eq!(
            rendered,
            ["  # type: integer", "  # minimum: 1", "  # maximum: 9"]
        );
    }

    #[test]
    fn the_null_type_is_quoted_and_a_null_enum_member_is_not() {
        assert_eq!(
            render(&json!({"type": ["string", "null"]}), 0, &Added::default()),
            ["# type: [string, 'null']"]
        );
        assert_eq!(
            render(&json!({"enum": ["a", null]}), 0, &Added::default()),
            ["# enum: [\"a\", null]"]
        );
    }

    #[test]
    fn a_field_yaml_would_read_as_a_boolean_is_quoted() {
        let rendered = render(
            &json!({"type": "object", "properties": {"on": {"type": "string"}}}),
            0,
            &Added::default(),
        );
        assert!(
            rendered.iter().any(|line| line.contains("\"on\":")),
            "{rendered:?}"
        );
    }

    #[test]
    fn a_struct_field_named_like_a_keyword_is_still_a_field() {
        let rendered = render(
            &json!({"type": "object", "properties": {"items": {"type": "string"}}}),
            0,
            &Added::default(),
        );
        assert_eq!(
            rendered,
            [
                "# type: object",
                "# properties:",
                "#   items:",
                "#     type: string",
            ]
        );
    }

    #[test]
    fn the_retired_enrolment_marker_is_refused_by_name() {
        let error = parse_markers("# @config-shape a.b generated\n", "x")
            .expect_err("the obsolete spelling is refused");
        assert!(error.to_string().contains("is obsolete"), "{error}");
    }

    #[test]
    fn a_transcription_names_the_release_and_the_source() {
        let (shapes, _) =
            parse_markers("# @config-shape a.b handwritten 8.9.1 src/config.rs\n", "x")
                .expect("the marker reads");
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].values_path, "a.b");
        assert!(shapes[0].handwritten);
        assert_eq!(shapes[0].version.as_deref(), Some("8.9.1"));
        assert_eq!(shapes[0].source.as_deref(), Some("src/config.rs"));

        assert!(
            parse_markers("# @config-shape a.b handwritten 8.9.1\n", "x")
                .expect_err("a transcription names both")
                .to_string()
                .contains("takes the release")
        );
    }

    #[test]
    fn a_departure_without_a_reason_is_refused() {
        let error = parse_markers("# @config-shape-except a.b items.id\n", "x")
            .expect_err("a departure carries a reason");
        assert!(error.to_string().contains("nobody can review"), "{error}");
    }

    #[test]
    fn a_marker_named_in_prose_is_not_a_declaration() {
        // Anchored as a whole comment line, so this file's own documentation and a chart's `# --`
        // description do not declare anything.
        let (shapes, divergences) = parse_markers(
            "# -- see @config-shape a.b handwritten 1 x for the escape hatch\n",
            "x",
        )
        .expect("prose reads");
        assert!(shapes.is_empty() && divergences.is_empty());
    }
}
