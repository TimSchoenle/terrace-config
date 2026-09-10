//! Checking one value, in the two steps `FORMAT.md` names — and asking the loader for the second.
//!
//! ```text
//! 1. form   the raw characters, against `text_constraint`   published, portable, unconditional
//! 2. range  the text *read*, against `constraint`           the loader's own, and only its own
//! ```
//!
//! Both are needed. Skipping the second leaves every bound in the document decorative: a pattern
//! matches characters, so `99999` is a well-formed integer and only a `maximum` catches it not
//! fitting a `u16`. Applying `constraint` to the raw text instead rejects `"0"` for an integer key,
//! which is a correct deployment refused.
//!
//! # The second step belongs to `producer.loader`, and a consumer has to ask
//!
//! `FORMAT.md`: the reads it tabulates are normative for `producer.loader == "figment"` "and for
//! nothing else. A consumer meeting a loader it does not know MUST skip step 2 and say so."
//!
//! The implementation this half was ported from performed both steps unconditionally and never read
//! `producer` at all — invisible while there was one producer, and wrong in the expensive direction
//! the moment there are two. A pattern *refusing* text the loader accepts stops a deployment that
//! was correct, and it does so at a gate with a false message rather than at boot with a true one.
//!
//! So the reads are a registry returning [`Option`], and the type is what makes the skip
//! unforgettable: there is no default row to fall through to and nothing to forget to branch on.
//! [`range`] hands back [`Range::NotChecked`] for a loader with no row, which the caller reports at
//! warning severity — never a pass, and never a failure.
//!
//! # Adding a row
//!
//! A row is a *measured* table, not a plausible one. Two things have to be true before one is added
//! here: the reads are published under a heading of their own in `spec/v1/FORMAT.md`, as figment's
//! are; and the implementation carries a test asserting its loader agrees with what this file says.
//! That dependency direction is what keeps the registry honest — the binary states the reads and
//! the implementations are held to them, rather than the binary guessing at loaders it cannot run.

use serde_json::{Map, Value as Json};

use crate::document::TextForm;
use crate::error::Error;

/// One loader's environment reads, as measured against that loader.
///
/// [`None`] from [`Self::parse`] means the loader cannot read this text as this form at all —
/// `"http"` as an integer — which is a finding about the value. It is distinct from
/// [`reads_for`] returning [`None`], which means nothing is known about how *any* text would be
/// read and is a finding about this build.
pub trait Reads: Sync {
    /// Read `text` as `form` names, or [`None`] when this loader cannot.
    fn parse(&self, form: TextForm, text: &str) -> Option<Json>;
}

/// The reads `spec/v1/FORMAT.md` publishes under `figment`.
///
/// Every read begins by trimming, because the environment layer trims before it parses anything —
/// for a plain string and a single character as much as for an integer. A read that skipped it
/// would refuse `" x "` against a `minLength` of 1, on a value that loads.
struct Figment;

impl Reads for Figment {
    #[expect(
        clippy::match_same_arms,
        reason = "`choice` and `text` read identically and are still not the same answer: one is trimmed and then held to the published `values`, the other is trimmed and finished. Folding them would make a future divergence a silent edit rather than a new arm."
    )]
    fn parse(&self, form: TextForm, text: &str) -> Option<Json> {
        let trimmed = text.trim();
        match form {
            // Not `str::parse`'s grammar and not TOML's: the table says trim, drop a leading `+`,
            // then read. `i64` rather than `u64` because a negative key is ordinary and the range
            // step is what holds the value to the type's own bounds.
            TextForm::Integer => trimmed
                .strip_prefix('+')
                .unwrap_or(trimmed)
                .parse::<i64>()
                .ok()
                .map(Json::from),
            // "compare to `true`", so `TRUE`, `1` and `yes` are all false rather than unreadable.
            // A boolean therefore always reads, and it is `constraint` — never this — that could
            // reject one.
            TextForm::Boolean => Some(Json::Bool(trimmed == "true")),
            TextForm::Choice => Some(Json::String(trimmed.to_owned())),
            TextForm::Structured => {
                // A TOML literal, and the brackets are the whole point: `A=a,b` reads like a list
                // and is not one, which is the deployment this form exists to name. Parsed as a
                // bare value: `toml`'s `FromStr` reads one directly, so nothing here has to invent
                // a surrounding document for it to sit in.
                if !is_toml_literal(trimmed) {
                    return None;
                }
                let value: toml::Value = trimmed.parse().ok()?;
                serde_json::to_value(&value).ok()
            }
            // A string is already its own value, and `unknown` never reaches here — `range` skips
            // it, because "nothing could be determined" is a gap rather than an answer.
            TextForm::Text | TextForm::Unknown => Some(Json::String(trimmed.to_owned())),
        }
    }
}

/// Whether the text is bracketed the way a TOML array or inline table is.
///
/// Checked before the parse rather than after it so the failure can say what was expected: a TOML
/// parser handed `a,b` reports a syntax error about a position, and the reader's actual mistake is
/// that they wrote a list without its brackets.
fn is_toml_literal(text: &str) -> bool {
    (text.starts_with('[') && text.ends_with(']')) || (text.starts_with('{') && text.ends_with('}'))
}

/// The loaders this build has a measured read table for.
///
/// `terrace-java` shares figment's row rather than carrying a copy of it: it implements the read
/// `FORMAT.md` publishes directly, which is what its tier 2 claim means. The day the two disagree
/// the shared row is wrong and must be split — and the test each implementation carries against
/// this table is what would say so, rather than a deployment failing at boot.
///
/// **`spring-boot` is deliberately absent.** Spring's `Binder` has its own relaxed-binding rules,
/// which is the case `CONFORMANCE.md` predicts by name, and nothing here has measured them. An
/// unmeasured row is worse than no row: no row skips the range check and says it skipped it, and a
/// wrong row refuses text the loader accepts. Add it when the reads are published and the Spring
/// side carries the test that holds its loader to them.
static REGISTRY: &[(&str, &(dyn Reads + 'static))] =
    &[("figment", &Figment), ("terrace-java", &Figment)];

/// The reads for one `producer.loader`, or [`None`] when this build has no measured table.
///
/// [`None`] is not a failure and not a default. It is the answer `FORMAT.md` requires: skip step 2,
/// and report it as skipped rather than passed.
pub fn reads_for(loader: &str) -> Option<&'static (dyn Reads + 'static)> {
    REGISTRY
        .iter()
        .find(|(name, _)| *name == loader)
        .map(|(_, reads)| *reads)
}

/// One key or declared external variable, as the union holds it.
///
/// Raw JSON rather than a struct, because the union's merge rule is that *every* field must agree
/// including ones this build has not learned, and a struct that dropped an unknown field would let
/// two contracts disagree about it in silence. The accessors here are the small typed window the
/// value checks need onto it.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a>(pub &'a Map<String, Json>);

impl<'a> Entry<'a> {
    /// The name this entry is reported under: a key's `path`, or an external variable's `name`.
    pub fn name(self) -> &'a str {
        self.0
            .get("path")
            .or_else(|| self.0.get("name"))
            .and_then(Json::as_str)
            .unwrap_or("?")
    }

    /// One field, whole.
    pub fn field(self, name: &str) -> Option<&'a Json> {
        self.0.get(name).filter(|value| !value.is_null())
    }

    /// One string field.
    pub fn text(self, name: &str) -> Option<&'a str> {
        self.field(name).and_then(Json::as_str)
    }

    /// The choices, when the type is a closed set.
    pub fn values(self) -> Vec<&'a str> {
        self.field("values")
            .and_then(Json::as_array)
            .map(|items| items.iter().filter_map(Json::as_str).collect())
            .unwrap_or_default()
    }

    /// How to read the text, refusing a form this build does not implement.
    ///
    /// A hard error rather than a silent downgrade to "skip both steps", and deliberately not the
    /// tolerant read [`TextForm`]'s own `Deserialize` performs: there, an unrecognised spelling
    /// becomes [`TextForm::Unknown`] so that one strange key cannot fail a whole document, which is
    /// right for rendering a table and wrong here. Under-checking a value while reporting success
    /// is the failure this half exists to remove, and the envelope version is the lever a producer
    /// has for saying the vocabulary changed.
    ///
    /// # Errors
    /// [`Error::Invalid`] when `text_form` is missing or names a form this build has never seen.
    pub fn text_form(self) -> Result<TextForm, Error> {
        let spelling = self.text("text_form").unwrap_or("");
        match spelling {
            "text" => Ok(TextForm::Text),
            "integer" => Ok(TextForm::Integer),
            "boolean" => Ok(TextForm::Boolean),
            "choice" => Ok(TextForm::Choice),
            "structured" => Ok(TextForm::Structured),
            "unknown" => Ok(TextForm::Unknown),
            other => Err(Error::Invalid(format!(
                "'{}' has text_form '{other}', which this build does not implement (known: \
                 boolean, choice, integer, structured, text, unknown)",
                self.name()
            ))),
        }
    }

    /// Whether a file can supply this key at all.
    ///
    /// The secrets directory and `_FILE` targets deliver their contents as strings with no parse,
    /// and no loader here coerces one into a number, a boolean or a TOML literal. So only a `text`
    /// key can be file-supplied, whatever the file contains — a chart mounting an integer key as a
    /// secret file has made a mistake no file contents can fix.
    ///
    /// `unknown` is refused too, and deliberately: nothing is known about how the loader will read
    /// it, so nothing says a raw string will do. That is the direction to be wrong in — a false
    /// report on a mount is one line in review, and a missing one is a credential silently unread.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the form is one this build does not implement.
    pub fn file_supplyable(self) -> Result<bool, Error> {
        Ok(self.text_form()? == TextForm::Text)
    }
}

/// Step 1 — the form: the raw characters, against `text_constraint`.
///
/// Unconditional, and the same for every loader: `text_constraint` is what the *producer* published
/// about its own reads, so a consumer holding text to it is repeating the producer's statement
/// rather than making one of its own.
///
/// `text_constraint: null` means only that there is no pattern to match — `text_form` is what says
/// whether that is because any text is correct or because nothing is known.
///
/// # Errors
/// [`Error::Invalid`] when the form or the constraint's vocabulary is one this build does not
/// implement.
pub fn form(entry: Entry<'_>, text: &str) -> Result<Option<String>, Error> {
    if entry.text_form()? == TextForm::Unknown {
        return Ok(None);
    }
    let Some(constraint) = entry.field("text_constraint") else {
        return Ok(None);
    };
    assert_value(constraint, &Json::String(text.to_owned()), "")
}

/// What step 2 had to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Range {
    /// The read succeeded and the value satisfies `constraint`, or there was nothing to check.
    Ok,
    /// The value is wrong, in a sentence naming what about it.
    Wrong(String),
    /// The check was not performed, and why. Never a pass and never a failure.
    NotChecked(String),
}

/// Step 2 — the range: the text read as `text_form` says, against `constraint`.
///
/// The only step a `minimum`, a `maximum` or a document-space `enum` is reachable from. Assumes
/// [`form`] already ran and passed, so text that does not satisfy the published pattern is
/// somebody else's report rather than a second line about the same value.
///
/// `loader` is `producer.loader`, or the empty string for a document that names none — which every
/// document written before the field existed does. Both take the same branch, because both mean the
/// same thing: nothing here knows how this text becomes a value.
///
/// # Errors
/// [`Error::Invalid`] when the form or the constraint's vocabulary is one this build does not
/// implement.
pub fn range(entry: Entry<'_>, loader: &str, text: &str) -> Result<Range, Error> {
    let form = entry.text_form()?;
    // `text` because a string is already its own value, `unknown` because nothing is known to read
    // it as. Decided before the loader is consulted: there is no read to perform for any loader, so
    // reporting one as unchecked would be reporting a gap that does not exist.
    //
    // `structured` is deliberately *not* here, and the distinction cost a real defect. A container
    // still has to be read before anything can be said about it, and the read is what says that
    // `a,b` is not a list — which is the deployment this form exists to name. Whether it is one is
    // the loader's question and not this crate's: figment wants a TOML literal, and a binder that
    // splits on commas reads the same text as two items. Skipping the read here made the check
    // silently unreachable for every loader at once.
    if matches!(form, TextForm::Text | TextForm::Unknown) {
        return Ok(Range::Ok);
    }

    let Some(reads) = reads_for(loader) else {
        return Ok(Range::NotChecked(unknown_loader(loader)));
    };

    let Some(value) = reads.parse(form, text) else {
        return Ok(Range::Wrong(unreadable(form, text)));
    };

    // The closed set is checked against the published `values` rather than left to `constraint`,
    // because a `choice` key whose constraint carries no `enum` would otherwise be unchecked. After
    // the read, so surrounding whitespace the loader trims is not a finding.
    if form == TextForm::Choice {
        let choices = entry.values();
        let read = value.as_str().unwrap_or_default();
        if !choices.is_empty() && !choices.contains(&read) {
            let allowed = choices
                .iter()
                .map(|choice| json_text(choice))
                .collect::<Vec<_>>()
                .join(", ");
            return Ok(Range::Wrong(format!(
                "{} is not one of {allowed}",
                json_text(text)
            )));
        }
    }

    let Some(constraint) = entry.field("constraint") else {
        return Ok(Range::Ok);
    };
    Ok(match assert_value(constraint, &value, "")? {
        Some(failure) => Range::Wrong(failure),
        None => Range::Ok,
    })
}

/// Why the range step was skipped, in a sentence that names the fix.
fn unknown_loader(loader: &str) -> String {
    let named = if loader.is_empty() {
        "the document names no `producer.loader`".to_owned()
    } else {
        format!("loader {} unknown to this build", json_text(loader))
    };
    format!(
        "range not checked: {named}, so the reads this document's `text_constraint` patterns were \
         measured against are not published here. The form was checked and the range was not — \
         performing it with another loader's rules would refuse text this one accepts, which stops \
         a deployment that was correct."
    )
}

/// What the loader could not read, in the form's own terms.
fn unreadable(form: TextForm, text: &str) -> String {
    let quoted = json_text(text);
    match form {
        TextForm::Integer => format!("{quoted} is not an integer"),
        TextForm::Boolean => format!(
            "{quoted} is not a boolean: the loader accepts `true` and `false`, and neither `TRUE` \
             nor `1` nor `yes`"
        ),
        TextForm::Structured => format!(
            "{quoted} is not a TOML array or inline table: a `structured` value carries its \
             brackets, so a list is [\"a\", \"b\"] and never a,b"
        ),
        _ => format!("{quoted} could not be read as this key's value"),
    }
}

/// One value, quoted the way JSON quotes it, for a message.
fn json_text(value: &str) -> String {
    Json::String(value.to_owned()).to_string()
}

// ------------------------------------------------------------------------------------------
// The constraint vocabulary
// ------------------------------------------------------------------------------------------

/// Keywords whose value is a scalar or a list of them: the whole vocabulary before
/// `schema_version: 2`.
const ASSERTIONS: &[&str] = &[
    "type",
    "enum",
    "const",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "pattern",
    "minLength",
    "maxLength",
];

/// Keywords whose value is itself a schema, or a mapping of them. The recursion points.
///
/// `additionalProperties` is the one that is two things at once: `true` and `false` are the
/// open/closed flag, and an *object* is a map's element schema. Both spellings are legal in the
/// same field and mean different things, so every reader branches on the type rather than the name.
const CONTAINERS: &[&str] = &["items", "additionalProperties", "properties"];

/// Flat keywords describing a container rather than a scalar, arriving with `schema_version: 2`.
const COLLECTIONS: &[&str] = &["required", "uniqueItems", "minItems", "maxItems"];

/// Keywords that say something to a reader and assert nothing about a value.
const ANNOTATIONS: &[&str] = &[
    "description",
    "title",
    "default",
    "examples",
    "$comment",
    "format",
];

/// Keywords of a constraint that are neither a flat assertion nor an annotation.
///
/// What a caller synthesising a value has to know: a keyword outside the flat vocabulary is one no
/// candidate walk can be steered by, so the key it belongs to is reported as unprobeable rather than
/// probed with a value nothing checked. The `schema_version: 2` container keywords are outside it on
/// purpose — a key carrying one describes a container, and a container's leaves are the operator's
/// own names.
#[must_use]
pub fn beyond_scalar_vocabulary(constraint: &Json) -> Vec<String> {
    let Some(schema) = constraint.as_object() else {
        return Vec::new();
    };
    let mut found: Vec<String> = schema
        .keys()
        .filter(|keyword| {
            !ASSERTIONS.contains(&keyword.as_str()) && !ANNOTATIONS.contains(&keyword.as_str())
        })
        .cloned()
        .collect();
    found.sort();
    found
}

/// Validate one value against the JSON Schema subset a contract may use.
///
/// Recurses, as of `schema_version: 2`: a container-typed key carries its element under `items` or
/// `additionalProperties`, and a walker that stopped at the top would report an array of the right
/// shape holding elements of the wrong one as correct.
///
/// A keyword outside the four sets is an error rather than a silent skip. Under-checking a value
/// while reporting success is the failure this whole half exists to remove, and a validator that
/// walked past a keyword it did not implement would be doing exactly that.
///
/// `at` names the position inside the value a failure was found at, so a caller's message still
/// points at something a person can edit: `[2].method` rather than the whole array.
///
/// # Errors
/// [`Error::Invalid`] when the constraint uses a keyword this build does not implement.
pub fn assert_value(constraint: &Json, value: &Json, at: &str) -> Result<Option<String>, Error> {
    let Some(schema) = constraint.as_object() else {
        return Ok(None);
    };

    let unsupported: Vec<&str> = {
        let mut found: Vec<&str> = schema
            .keys()
            .map(String::as_str)
            .filter(|keyword| {
                !ASSERTIONS.contains(keyword)
                    && !CONTAINERS.contains(keyword)
                    && !COLLECTIONS.contains(keyword)
                    && !ANNOTATIONS.contains(keyword)
            })
            .collect();
        found.sort_unstable();
        found
    };
    if !unsupported.is_empty() {
        return Err(Error::Invalid(format!(
            "constraint uses keywords this validator does not implement: {}",
            unsupported.join(", ")
        )));
    }

    if let Some(failure) = flat(schema, value) {
        return Ok(Some(position(at, &failure)));
    }
    container(schema, value, at)
}

/// The keywords whose value is a scalar: the whole vocabulary before `schema_version: 2`.
fn flat(schema: &Map<String, Json>, value: &Json) -> Option<String> {
    if let Some(declared) = schema.get("type")
        && !is_type(value, declared)
    {
        return Some(format!("expected {declared}, got {value}"));
    }
    if let Some(Json::Array(choices)) = schema.get("enum")
        && !choices.contains(value)
    {
        let allowed = choices
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        return Some(format!("{value} is not one of {allowed}"));
    }
    if let Some(constant) = schema.get("const")
        && value != constant
    {
        return Some(format!("{value} is not {constant}"));
    }

    // A bound on a boolean is meaningless, and JSON's `true` is not a number — so unlike Python,
    // where `bool` is an `int`, there is no trap to remember here.
    if let Some(number) = value.as_f64().filter(|_| !value.is_boolean())
        && let Some(failure) = bounds(schema, value, number)
    {
        return Some(failure);
    }

    if let Some(text) = value.as_str() {
        if let Some(Json::String(pattern)) = schema.get("pattern") {
            // Unanchored, as JSON Schema's `pattern` is. Printed as written rather than quoted: it
            // is full of backslashes, and doubling every one makes the line a reader compares their
            // value against harder to read than the value itself.
            match regex::Regex::new(pattern) {
                Ok(compiled) if !compiled.is_match(text) => {
                    return Some(format!("{value} does not match {pattern}"));
                }
                // A pattern this engine cannot compile is the producer's statement about its own
                // reads, and refusing the value over it would fail a deployment for a defect in the
                // document. The document gate reports the pattern itself.
                Ok(_) | Err(_) => {}
            }
        }
        let length = text.chars().count() as u64;
        if let Some(minimum) = schema.get("minLength").and_then(Json::as_u64)
            && length < minimum
        {
            return Some(format!("{value} is shorter than {minimum} characters"));
        }
        if let Some(maximum) = schema.get("maxLength").and_then(Json::as_u64)
            && length > maximum
        {
            return Some(format!("{value} is longer than {maximum} characters"));
        }
    }

    None
}

/// The numeric bounds, in the order a reader would apply them.
fn bounds(schema: &Map<String, Json>, value: &Json, number: f64) -> Option<String> {
    for (keyword, phrasing, failed) in [
        (
            "minimum",
            "is below the minimum",
            (|number, bound| number < bound) as fn(f64, f64) -> bool,
        ),
        ("maximum", "is above the maximum", |number, bound| {
            number > bound
        }),
        ("exclusiveMinimum", "is not above", |number, bound| {
            number <= bound
        }),
        ("exclusiveMaximum", "is not below", |number, bound| {
            number >= bound
        }),
    ] {
        if let Some(bound) = schema.get(keyword)
            && let Some(as_number) = bound.as_f64()
            && failed(number, as_number)
        {
            return Some(format!("{value} {phrasing} {bound}"));
        }
    }
    if let Some(factor) = schema.get("multipleOf")
        && let Some(as_number) = factor.as_f64()
        && as_number != 0.0
        && (number % as_number) != 0.0
    {
        return Some(format!("{value} is not a multiple of {factor}"));
    }
    None
}

/// The `schema_version: 2` half: a container's own bounds, then each of its elements.
///
/// Every keyword here is skipped when the value is not the shape it applies to, exactly as the
/// numeric and string bounds are. A `minItems` on a value that is not a list is not a failure of
/// that value — `type` is what says the value is the wrong shape, and reporting the same defect
/// twice makes the first line harder to find.
fn container(schema: &Map<String, Json>, value: &Json, at: &str) -> Result<Option<String>, Error> {
    if let Json::Array(items) = value {
        if let Some(minimum) = schema.get("minItems").and_then(Json::as_u64)
            && (items.len() as u64) < minimum
        {
            return Ok(Some(position(
                at,
                &format!("has {} item(s), fewer than {minimum}", items.len()),
            )));
        }
        if let Some(maximum) = schema.get("maxItems").and_then(Json::as_u64)
            && (items.len() as u64) > maximum
        {
            return Ok(Some(position(
                at,
                &format!("has {} item(s), more than {maximum}", items.len()),
            )));
        }
        if schema.get("uniqueItems") == Some(&Json::Bool(true)) && !unique(items) {
            return Ok(Some(position(
                at,
                "has repeated items, and every item has to be distinct",
            )));
        }
        if let Some(element) = schema.get("items").filter(|element| element.is_object()) {
            for (index, item) in items.iter().enumerate() {
                if let Some(failure) = assert_value(element, item, &format!("{at}[{index}]"))? {
                    return Ok(Some(failure));
                }
            }
        }
    }

    if let Json::Object(fields) = value {
        if let Some(Json::Array(required)) = schema.get("required") {
            for name in required {
                if let Some(name) = name.as_str()
                    && !fields.contains_key(name)
                {
                    return Ok(Some(position(
                        at,
                        &format!("is missing the required key {}", json_text(name)),
                    )));
                }
            }
        }

        let properties = schema.get("properties").and_then(Json::as_object);
        if let Some(properties) = properties {
            for (name, subschema) in properties {
                if let (Some(held), true) = (fields.get(name), subschema.is_object())
                    && let Some(failure) = assert_value(subschema, held, &format!("{at}.{name}"))?
                {
                    return Ok(Some(failure));
                }
            }
        }

        // Only the object spelling is an element schema. `true` and `false` are the open/closed
        // flag, and neither says anything about a value — the union is where closure is decided,
        // and it decides it for the document rather than for one key.
        if let Some(element) = schema
            .get("additionalProperties")
            .filter(|element| element.is_object())
        {
            for (name, held) in fields {
                if properties.is_some_and(|declared| declared.contains_key(name)) {
                    continue;
                }
                if let Some(failure) = assert_value(element, held, &format!("{at}.{name}"))? {
                    return Ok(Some(failure));
                }
            }
        }
    }

    Ok(None)
}

/// Whether every item differs, by value.
///
/// Quadratic, over arrays whose length is bounded by what somebody typed into a values file. A
/// contract's elements are arbitrary JSON, which is not hashable, and the readable implementation
/// is the right one at this size.
fn unique(items: &[Json]) -> bool {
    for (index, item) in items.iter().enumerate() {
        if items[index + 1..].contains(item) {
            return false;
        }
    }
    true
}

/// One failure, prefixed by its position inside the value when it has one.
///
/// Empty at the top level, where the caller has already named the key and a second name for the
/// same thing is noise.
fn position(at: &str, message: &str) -> String {
    if at.is_empty() {
        message.to_owned()
    } else {
        format!("{at}: {message}")
    }
}

/// Whether a value satisfies a JSON Schema `type`, scalar or list.
pub fn is_type(value: &Json, declared: &Json) -> bool {
    let names: Vec<&Json> = match declared {
        Json::Array(items) => items.iter().collect(),
        single => vec![single],
    };
    names
        .into_iter()
        .filter_map(Json::as_str)
        .any(|name| match name {
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "string" => value.is_string(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            "null" => value.is_null(),
            _ => false,
        })
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value as Json, json};

    use super::{
        ASSERTIONS, Entry, Range, assert_value, beyond_scalar_vocabulary, form, range, reads_for,
    };
    use crate::document::TextForm;

    fn entry(value: &Json) -> Map<String, Json> {
        value.as_object().expect("an entry is an object").clone()
    }

    #[test]
    fn an_unknown_loader_skips_the_range_and_says_so() {
        let held = entry(&json!({
            "path": "server.port", "text_form": "integer",
            "constraint": {"type": "integer", "maximum": 65535},
        }));
        let found = range(Entry(&held), "spring-boot", "99999").expect("the form is known");
        let Range::NotChecked(message) = found else {
            panic!("a loader with no measured row must not be checked: {found:?}");
        };
        assert!(
            message.starts_with("range not checked: loader \"spring-boot\" unknown to this build"),
            "{message}"
        );
    }

    #[test]
    fn a_document_naming_no_loader_takes_the_same_branch() {
        let held =
            entry(&json!({"path": "a", "text_form": "integer", "constraint": {"maximum": 1}}));
        let found = range(Entry(&held), "", "9").expect("the form is known");
        assert!(matches!(found, Range::NotChecked(_)), "{found:?}");
    }

    #[test]
    fn a_known_loader_reaches_the_bound_a_pattern_cannot() {
        let held = entry(&json!({
            "path": "server.port", "text_form": "integer",
            "text_constraint": {"type": "string", "pattern": "^[0-9]+$"},
            "constraint": {"type": "integer", "maximum": 65535},
        }));
        // The form passes: 99999 is a well-formed integer, which is the whole reason step 2 exists.
        assert_eq!(
            form(Entry(&held), "99999").expect("the form is known"),
            None
        );
        let Range::Wrong(message) = range(Entry(&held), "figment", "99999").expect("readable")
        else {
            panic!("99999 is above a u16's maximum");
        };
        assert!(message.contains("above the maximum 65535"), "{message}");
    }

    #[test]
    fn text_the_loader_cannot_read_is_a_finding_about_the_value() {
        let held = entry(&json!({"path": "a", "text_form": "integer"}));
        let Range::Wrong(message) =
            range(Entry(&held), "figment", "http").expect("the form is known")
        else {
            panic!("`http` is not an integer");
        };
        assert_eq!(message, "\"http\" is not an integer");
    }

    #[test]
    fn a_form_with_no_range_is_not_reported_as_unchecked() {
        // `text` has no range for *any* loader, so an unknown one changes nothing about it and
        // saying "not checked" would report a gap that does not exist.
        let held = entry(&json!({"path": "a", "text_form": "text"}));
        assert_eq!(
            range(Entry(&held), "nobody", "anything").expect("the form is known"),
            Range::Ok
        );
    }

    #[test]
    fn the_loader_trims_before_it_reads() {
        let reads = reads_for("figment").expect("figment has a measured row");
        assert_eq!(reads.parse(TextForm::Integer, " +42 "), Some(json!(42)));
        assert_eq!(reads.parse(TextForm::Boolean, " true "), Some(json!(true)));
        assert_eq!(
            reads.parse(TextForm::Structured, " [1, 2] "),
            Some(json!([1, 2]))
        );
    }

    #[test]
    fn a_list_without_its_brackets_is_the_defect_structured_exists_to_name() {
        let reads = reads_for("figment").expect("figment has a measured row");
        assert_eq!(reads.parse(TextForm::Structured, "a,b"), None);
    }

    #[test]
    fn a_choice_is_held_to_the_published_values_after_the_trim() {
        let held = entry(&json!({"path": "a", "text_form": "choice", "values": ["json", "text"]}));
        assert_eq!(
            range(Entry(&held), "figment", " json ").expect("readable"),
            Range::Ok
        );
        let Range::Wrong(message) = range(Entry(&held), "figment", "yaml").expect("readable")
        else {
            panic!("`yaml` is not one of the choices");
        };
        assert_eq!(message, "\"yaml\" is not one of \"json\", \"text\"");
    }

    #[test]
    fn a_keyword_this_validator_does_not_implement_is_an_error_not_a_skip() {
        let error = assert_value(&json!({"allOf": []}), &json!(1), "")
            .expect_err("an unimplemented keyword is refused");
        assert!(error.to_string().contains("allOf"), "{error}");
    }

    #[test]
    fn an_element_of_the_wrong_shape_is_found_and_named_by_position() {
        let constraint = json!({"type": "array", "items": {"enum": ["get", "post"]}});
        let failure = assert_value(&constraint, &json!(["get", "put"]), "")
            .expect("the vocabulary is implemented")
            .expect("`put` is not one of the choices");
        assert!(failure.starts_with("[1]: "), "{failure}");
    }

    #[test]
    fn an_unreadable_text_form_is_refused_rather_than_read_as_unknown() {
        let held = entry(&json!({"path": "a", "text_form": "duration"}));
        let error = Entry(&held)
            .text_form()
            .expect_err("a form this build has not implemented is refused");
        assert!(error.to_string().contains("duration"), "{error}");
    }

    #[test]
    fn every_flat_assertion_a_contract_may_use_is_inside_the_scalar_vocabulary() {
        // A keyword a contract is accepted for carrying and a value synthesiser refuses would be
        // an unexplained skip: a key with no probe, for a reason nothing in the document says.
        for keyword in ASSERTIONS {
            let schema = json!({*keyword: 1, "type": "integer"});
            assert!(
                !beyond_scalar_vocabulary(&schema).contains(&(*keyword).to_owned()),
                "{keyword}"
            );
        }
    }

    #[test]
    fn a_container_keyword_is_outside_it_and_an_annotation_is_not() {
        // The container keywords belong to a key that describes a container, whose leaves are the
        // operator's own names — so a caller synthesising a value has nothing to synthesise. An
        // annotation asserts nothing, so it steers nothing and refuses nothing.
        assert_eq!(
            beyond_scalar_vocabulary(&json!({"type": "array", "items": {}, "minItems": 1})),
            ["items", "minItems"]
        );
        assert!(beyond_scalar_vocabulary(&json!({"type": "string", "default": "x"})).is_empty());
        assert!(beyond_scalar_vocabulary(&json!(null)).is_empty());
    }
}
