//! Reading a chart's vendored contracts back out, and saying what its images actually consume.
//!
//! Every other consumer of a chart's contracts is a gate: it takes a rendered manifest, holds it
//! against the contract and reports the difference. Nothing reads the contract for its own sake — so
//! the one document that states, per setting, what the binary calls it, what it accepts, what it
//! defaults to and whether a file may supply it is only ever consulted by a program. An operator
//! asking "what can I put in this file, and why is the value I set being ignored?" has the chart's
//! values, the chart's README and a 400 KB JSON file, and the answer is in the JSON file.
//!
//! This is that file, read out loud. Offline, read-only, no render, no network: it opens the same
//! committed contracts the gates do, through the same declaration and the same staleness interlock.
//!
//! Three decisions are worth stating, because each is a place where the obvious implementation says
//! something untrue.
//!
//! **Readers are derived, never listed.** A chart pinning nine images across nine documents states
//! nowhere which of them reads a given setting — not in the chart, not in the declaration, not in
//! the README. It is also the fact this exists to produce: a key read by three services and another
//! by all nine is what tells an operator whether a change is local or fleet-wide. So the attribution
//! is computed by walking the contracts the declaration binds, and a hand-written map is exactly the
//! thing that would be wrong within one release.
//!
//! **The merge here is tolerant where [`crate::union`] is strict, and that is not a relaxation of
//! the rule.** A union refuses two contracts that describe one key differently, because its output
//! validates a document all of them read, and an unreconcilable pair there means the gate has
//! nothing trustworthy to say. It cannot be reused across a chart's *documents*: nine separate files
//! legitimately carry nine different titles, and merging them is not a question anyone asked. What
//! this wants is the opposite posture — a disagreement between two images about one setting is a
//! finding, not a reason to print nothing — so the merge below keeps every image's description,
//! shows the divergence, and reports it as a warning. No gate can currently see those.
//!
//! **Loader variables get a section of their own**, beside the external ones. They are neither
//! settings nor foreign variables: they are what decides which file the settings come from, and an
//! explanation of a configuration surface that omits how the surface is located is an explanation an
//! operator cannot act on.
//!
//! Deliberately omitted: the merged JSON Schema. It is the same information as the key list in a
//! form built for a validator rather than a person, and printing it would bury the part that answers
//! the question.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value as Json};

use crate::error::Error;
use crate::report::{Report, warning};
use crate::union::UNKNOWN_POLICIES;
use crate::value::Entry;

use super::declaration::{Declaration, bind, load_vendored};

/// Width the prose is re-wrapped to.
///
/// Documentation arrives hard-wrapped at the producer's own width, which is not this one, so
/// paragraphs are re-flowed rather than printed as they came.
pub const WIDTH: usize = 96;

/// Column ceilings for the compact listing.
///
/// The columns size themselves to the selection — a filtered listing is narrower than the whole
/// chart — and these only stop one 60-character type from setting the width for 167 rows.
const MAX_PATH: usize = 46;
const MAX_TYPE: usize = 30;
const MAX_DEFAULT: usize = 22;

/// Fields whose representative value is the first *non-empty* one across the images that read a
/// setting.
///
/// An image that says nothing about a key it shares has not contradicted the one that does.
const PROSE_FIELDS: [&str; 2] = ["docs", "note"];

/// Fields a union merges rather than compares, so a difference in one is not a divergence.
const UNIONED_FIELDS: [&str; 1] = ["required"];

/// Constraint keywords rendered as prose, in the order they read best.
///
/// Anything outside this list is printed as `keyword=<json>` rather than dropped: under-reporting a
/// constraint is the failure this whole pipeline exists to remove, and it would be no better coming
/// from the explainer.
const CONSTRAINT_ORDER: [&str; 19] = [
    "type",
    "const",
    "enum",
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

/// Keywords folded into another keyword's phrase, or said elsewhere in the entry.
///
/// Listed so the catch-all can tell "already reported" from "unrecognised".
const CONSTRAINT_FOLDED: [&str; 11] = [
    "exclusiveMinimum",
    "exclusiveMaximum",
    "maximum",
    "maxLength",
    "maxItems",
    "required",
    "description",
    "title",
    "default",
    "examples",
    "$comment",
];

/// One image a chart pins, named as the declaration names it.
///
/// The name is the vendored contract's stem rather than the repository part of the image reference.
/// That is what the declaration writes and what a maintainer edits, it is short enough to list nine
/// of on one line, and it maps one-to-one to an image — where a full repository path does not fit
/// and repeats the chart name once per service. The full reference is in the header and in the JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reader {
    /// The vendored contract's stem.
    pub name: String,
    /// Its file name.
    pub contract: String,
    /// The image it describes.
    pub image: String,
    /// The digest it was published for.
    pub digest: String,
    /// The application's own name.
    pub app: String,
    /// The release.
    pub version: String,
    /// The declared documents it reads.
    pub documents: Vec<String>,
}

/// One setting, and every image that declared it.
///
/// Held as the occurrences rather than as a merged entry because the divergence between two images'
/// descriptions is a thing to print, not a thing to resolve.
#[derive(Debug, Clone, Default)]
pub struct Setting {
    /// The path, variable or name it is keyed on.
    pub name: String,
    /// Each image that declared it, and what it said.
    pub occurrences: Vec<(String, Map<String, Json>)>,
}

impl Setting {
    /// The images that declared it, in the order they were absorbed.
    #[must_use]
    pub fn readers(&self) -> Vec<&str> {
        self.occurrences
            .iter()
            .map(|(reader, _)| reader.as_str())
            .collect()
    }

    /// The value to show for one field, across every image that declared this setting.
    ///
    /// `required` unions, which is the merge rule restated: a setting any reader requires must be
    /// present, so an image that does not require it cannot make it optional. Prose takes the first
    /// non-empty. Everything else takes the first image's, and any disagreement is reported
    /// separately rather than silently resolved here.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<&Json> {
        if name == "required" {
            let any = self
                .occurrences
                .iter()
                .any(|(_, entry)| entry.get("required").and_then(Json::as_bool) == Some(true));
            return Some(if any {
                &Json::Bool(true)
            } else {
                &Json::Bool(false)
            });
        }
        if PROSE_FIELDS.contains(&name) {
            return self
                .occurrences
                .iter()
                .find_map(|(_, entry)| entry.get(name).filter(|held| !is_empty(held)));
        }
        self.occurrences
            .first()
            .and_then(|(_, entry)| entry.get(name))
    }

    /// One entry standing for all of them, shaped exactly as a published entry is.
    ///
    /// Shaped that way on purpose: the typed readings of an entry are normative, and handing them a
    /// merged map rather than reimplementing their rules is what keeps this agreeing with the gates.
    ///
    /// Its fields come out in the document model's order, which is alphabetical — the same property
    /// that makes a document this build publishes a byte-for-byte round trip through its own reader.
    /// The implementation this was ported from kept the order the producer happened to write, to the
    /// same end: that the JSON here diffs cleanly against the vendored file. Against a document
    /// written by that older producer the two orders differ; against one written by this build they
    /// are the same order, and this one is the same on every machine.
    #[must_use]
    pub fn representative(&self) -> Map<String, Json> {
        let mut names: Vec<&str> = Vec::new();
        for (_, entry) in &self.occurrences {
            for name in entry.keys() {
                if !names.contains(&name.as_str()) {
                    names.push(name);
                }
            }
        }
        names
            .into_iter()
            .map(|name| {
                (
                    name.to_owned(),
                    self.value(name).cloned().unwrap_or(Json::Null),
                )
            })
            .collect()
    }

    /// Each distinct value of one field, with the images that published it.
    #[must_use]
    pub fn variants(&self, name: &str) -> Vec<(Json, Vec<&str>)> {
        let mut seen: Vec<(Json, Vec<&str>)> = Vec::new();
        for (reader, entry) in &self.occurrences {
            let value = entry.get(name).cloned().unwrap_or(Json::Null);
            match seen.iter_mut().find(|(known, _)| known == &value) {
                Some((_, readers)) => readers.push(reader),
                None => seen.push((value, vec![reader])),
            }
        }
        seen
    }

    /// Fields two images describe differently. `required` is excluded: it unions by rule.
    #[must_use]
    pub fn divergent(&self) -> Vec<String> {
        let mut fields: BTreeSet<&str> = BTreeSet::new();
        for (_, entry) in &self.occurrences {
            fields.extend(entry.keys().map(String::as_str));
        }
        fields
            .into_iter()
            .filter(|name| !UNIONED_FIELDS.contains(name) && self.variants(name).len() > 1)
            .map(str::to_owned)
            .collect()
    }
}

/// Everything one chart's images read, merged across every document the chart declares.
#[derive(Debug, Default)]
pub struct Surface {
    /// The chart.
    pub chart: String,
    /// The spelling rules the first image absorbed uses.
    pub dialect: Map<String, Json>,
    /// The images.
    pub readers: Vec<Reader>,
    /// Every configuration key, by path.
    pub keys: BTreeMap<String, Setting>,
    /// Every loader variable, by name.
    pub loader: BTreeMap<String, Setting>,
    /// Every declared external variable, by name.
    pub external: BTreeMap<String, Setting>,
    /// The ignore patterns, in the order they were absorbed.
    pub ignore: Vec<String>,
    /// The strictest policy any image declared.
    pub unknown: String,
}

/// Bind every declared document, then read the contracts binding proved belong to it.
///
/// The binding is called for its interlock and for nothing else. It answers whether the vendored
/// file is for the digest the chart pins, which is the only question that decides whether anything
/// printed below describes the deployed image — and it is answered here exactly as the gates answer
/// it, so this cannot report facts a gate would refuse to trust. What it returns is shaped for
/// validation and carries no provenance, so the contracts are re-read afterwards for the image
/// reference and the release. That is a second read of a file already proven, not a second opinion
/// about it.
///
/// [`None`] means the interlock refused: a contract that is not for the digest the chart pins
/// describes some other build, and printing its settings as this chart's would be a confident wrong
/// answer to the only question anyone runs this to ask.
///
/// # Errors
/// [`Error::Invalid`] when a contract cannot be read at all, [`Error::Io`] when the chart cannot be.
pub fn collect(
    chart_dir: &Path,
    declaration: &Declaration,
    report: &mut Report,
) -> Result<Option<Surface>, Error> {
    let values = super::read_file(&chart_dir.join("values.yaml"))?;
    let chart_yaml = super::read_file(&chart_dir.join("Chart.yaml"))?;
    let app_version = chart_yaml.get("appVersion").and_then(Json::as_str);

    let mut surface = Surface {
        chart: declaration.chart.clone(),
        unknown: "reject".to_owned(),
        ..Surface::default()
    };
    let mut by_contract: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut refused = false;

    for document in &declaration.documents {
        let at = format!("{}: {}", declaration.chart, document.name);
        let (binding, problems) = bind(chart_dir, document, &values, app_version)?;
        for problem in problems {
            report.fail(&at, problem);
        }
        if binding.is_none() {
            refused = true;
            continue;
        }
        for reference in &document.images {
            by_contract
                .entry(reference.contract.clone())
                .or_default()
                .push(document.name.clone());
        }
    }
    if refused {
        return Ok(None);
    }

    // Sorted, so the order every reader list and every "the images disagree" side is printed in is a
    // property of the contracts rather than of where a maintainer happened to add a document.
    for (contract_path, documents) in by_contract {
        let vendored = load_vendored(&chart_dir.join(&contract_path))?;
        let name = Path::new(&contract_path)
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or(&contract_path)
            .to_owned();
        absorb(&mut surface, &vendored, &name, documents, report);
    }
    Ok(Some(surface))
}

/// Fold one image's contract into the chart-wide surface.
fn absorb(
    surface: &mut Surface,
    vendored: &super::declaration::Vendored,
    name: &str,
    documents: Vec<String>,
    report: &mut Report,
) {
    let contract = &vendored.contract;
    let app = contract.get("app");
    surface.readers.push(Reader {
        name: name.to_owned(),
        contract: vendored
            .path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default()
            .to_owned(),
        image: vendored.image.clone(),
        digest: vendored.digest.clone(),
        app: app
            .and_then(|app| app.get("name"))
            .and_then(Json::as_str)
            .unwrap_or(name)
            .to_owned(),
        version: app
            .and_then(|app| app.get("version"))
            .and_then(Json::as_str)
            .unwrap_or("?")
            .to_owned(),
        documents,
    });

    let dialect = contract
        .get("schema")
        .and_then(|schema| schema.get("dialect"))
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let policy = contract
        .get("external")
        .and_then(|external| external.get("unknown"))
        .and_then(Json::as_str)
        .unwrap_or("reject")
        .to_owned();

    if surface.dialect.is_empty() {
        surface.dialect = dialect;
        surface.unknown = policy.clone();
    } else if surface.dialect != dialect {
        // A union makes this fatal, and per document it must be: two images reading one file under
        // different spelling rules do not share a namespace. Across documents it is merely
        // surprising — nine separate files could legitimately use nine prefixes — so it is said
        // rather than raised, and the header reports the first image's dialect.
        report.add(
            surface.chart.clone(),
            warning(format!(
                "{name} reads its configuration under the dialect {}, where the other images this \
                 chart pins use {}; the header below reports the latter",
                Json::Object(dialect),
                Json::Object(surface.dialect.clone())
            )),
        );
    }

    for (section, field, keyed_on) in [
        ("keys", "schema", "path"),
        ("loader", "schema", "env"),
        ("env", "external", "name"),
    ] {
        let held = contract
            .get(field)
            .and_then(|block| block.get(section))
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default();
        let into = match section {
            "keys" => &mut surface.keys,
            "loader" => &mut surface.loader,
            _ => &mut surface.external,
        };
        for entry in held {
            record(into, &entry, keyed_on, name);
        }
    }

    for pattern in contract
        .get("external")
        .and_then(|external| external.get("ignore"))
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
    {
        if !surface.ignore.iter().any(|held| held == pattern) {
            surface.ignore.push(pattern.to_owned());
        }
    }

    let rank = |policy: &str| UNKNOWN_POLICIES.iter().position(|held| *held == policy);
    if rank(&policy) > rank(&surface.unknown) {
        surface.unknown = policy;
    }
}

/// Record one published entry against the name it is keyed on.
///
/// An entry with no name is skipped rather than refused: it is a contract this build cannot read,
/// and the gates are what say so. Printing every other setting is more useful than printing none.
fn record(into: &mut BTreeMap<String, Setting>, entry: &Json, keyed_on: &str, reader: &str) {
    let (Some(name), Some(fields)) = (
        entry.get(keyed_on).and_then(Json::as_str),
        entry.as_object(),
    ) else {
        return;
    };
    into.entry(name.to_owned())
        .or_insert_with(|| Setting {
            name: name.to_owned(),
            occurrences: Vec::new(),
        })
        .occurrences
        .push((reader.to_owned(), fields.clone()));
}

/// Say where two images describe one setting differently, within the selection.
///
/// Nothing else can see these. Each of a chart's documents is validated against its own images'
/// union, so two images that never share a document never have their descriptions of a shared
/// setting compared.
///
/// Scoped to the selection, because a warning about a setting the reader did not ask about is noise
/// on every run and would train them to ignore the one that matters.
pub fn report_divergences(surface: &Surface, pattern: Option<&str>, report: &mut Report) {
    for (section, settings) in [
        ("key", &surface.keys),
        ("loader variable", &surface.loader),
        ("external variable", &surface.external),
    ] {
        for setting in select(settings, pattern) {
            for name in setting.divergent() {
                let sides: Vec<String> = setting
                    .variants(&name)
                    .into_iter()
                    .map(|(value, readers)| format!("{}: {}", readers.join(", "), terse(&value)))
                    .collect();
                report.add(
                    &surface.chart,
                    warning(format!(
                        "the images pinned here describe the {section} {} differently: `{name}` \
                         is {}",
                        crate::gate::quoted(&setting.name),
                        sides.join("; ")
                    )),
                );
            }
        }
    }
}

/// One value, clipped to the width a finding can carry.
fn terse(value: &Json) -> String {
    let text = value.to_string();
    if text.chars().count() <= 60 {
        text
    } else {
        format!("{}...", text.chars().take(60).collect::<String>())
    }
}

/// The settings a pattern names, in path order.
///
/// A pattern carrying a glob metacharacter is matched as a glob against the whole name; anything
/// else is a case-insensitive substring, because the name an operator has in hand is usually a
/// fragment of a path they read in a values file. Both are anchored on the name alone — the
/// environment spelling is derived from it, so a pattern that matches the path matches the variable
/// a reader is actually holding.
#[must_use]
pub fn select<'a>(
    settings: &'a BTreeMap<String, Setting>,
    pattern: Option<&str>,
) -> Vec<&'a Setting> {
    let chosen = settings.values();
    let Some(pattern) = pattern.filter(|held| !held.is_empty()) else {
        return chosen.collect();
    };
    if pattern.contains(['*', '?', '[']) {
        return chosen.filter(|item| glob(pattern, &item.name)).collect();
    }
    let lowered = pattern.to_lowercase();
    chosen
        .filter(|item| item.name.to_lowercase().contains(&lowered))
        .collect()
}

/// A shell-style glob over one name: `*`, `?` and a `[...]` set.
///
/// Hand-rolled because it is twenty lines and the alternative is a dependency carried into every
/// build for one command's argument. Case-sensitive, as the implementation this was ported from is.
fn glob(pattern: &str, name: &str) -> bool {
    let (pattern, name): (Vec<char>, Vec<char>) =
        (pattern.chars().collect(), name.chars().collect());
    matches(&pattern, &name)
}

fn matches(pattern: &[char], name: &[char]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some('*') => {
            // Every split of the remaining name, shortest suffix first. A pattern of one `*` is the
            // common case and finds its answer immediately.
            (0..=name.len()).any(|at| matches(&pattern[1..], &name[at..]))
        }
        Some('?') => !name.is_empty() && matches(&pattern[1..], &name[1..]),
        Some('[') => {
            let Some(close) = pattern
                .iter()
                .position(|held| *held == ']')
                .filter(|at| *at > 1)
            else {
                // An unclosed bracket is a literal one, which is what a shell does with it.
                return !name.is_empty() && name[0] == '[' && matches(&pattern[1..], &name[1..]);
            };
            if name.is_empty() {
                return false;
            }
            let (negated, from) = if pattern[1] == '!' {
                (true, 2)
            } else {
                (false, 1)
            };
            let held = in_set(&pattern[from..close], name[0]);
            held != negated && matches(&pattern[close + 1..], &name[1..])
        }
        Some(literal) => {
            !name.is_empty() && name[0] == *literal && matches(&pattern[1..], &name[1..])
        }
    }
}

/// Whether one character is in a bracket set, ranges included.
fn in_set(set: &[char], held: char) -> bool {
    let mut at = 0;
    while at < set.len() {
        if at + 2 < set.len() && set[at + 1] == '-' {
            if set[at] <= held && held <= set[at + 2] {
                return true;
            }
            at += 3;
            continue;
        }
        if set[at] == held {
            return true;
        }
        at += 1;
    }
    false
}

/// Whether a published value says nothing.
fn is_empty(value: &Json) -> bool {
    match value {
        Json::Null => true,
        Json::String(text) => text.is_empty(),
        Json::Array(items) => items.is_empty(),
        Json::Object(fields) => fields.is_empty(),
        _ => false,
    }
}

// ------------------------------------------------------------------------------------------------
// Rendering
// ------------------------------------------------------------------------------------------------

/// A JSON Schema constraint object, as a phrase.
///
/// Bounds are folded into one range because that is how they are read — `0 to 65535`, not `integer,
/// minimum 0, maximum 65535` — and an unrecognised keyword is printed rather than dropped, so a
/// vocabulary this renderer does not know still reaches the reader.
///
/// Recurses, as of `schema_version: 2`. A container-typed key carries what one element holds under
/// `items` or `additionalProperties`, and printing that as raw JSON turns the one line an operator
/// reads about a setting into a wall of braces. `array of string one of "GET" | "POST"` is the same
/// fact.
///
/// A struct element is named by its fields rather than described in full: a route configuration has
/// twenty of them, each with a type and a bound of its own, and a table cell is not where that
/// belongs. The JSON form emits the constraint verbatim for a reader that wants all of it.
#[must_use]
pub fn describe_constraint(constraint: Option<&Json>) -> String {
    let Some(schema) = constraint
        .and_then(Json::as_object)
        .filter(|held| !held.is_empty())
    else {
        return String::new();
    };

    let mut parts: Vec<String> = Vec::new();
    for keyword in CONSTRAINT_ORDER {
        let Some(value) = schema.get(keyword) else {
            continue;
        };
        match keyword {
            "type" => parts.push(match value {
                Json::String(name) => name.clone(),
                Json::Array(names) => names
                    .iter()
                    .map(|name| name.as_str().unwrap_or_default().to_owned())
                    .collect::<Vec<_>>()
                    .join(" or "),
                other => other.to_string(),
            }),
            "const" => parts.push(format!("exactly {value}")),
            "enum" => parts.push(format!(
                "one of {}",
                value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" | ")
            )),
            "pattern" => parts.push(format!("matching {}", plain(value))),
            "format" => parts.push(format!("format {}", plain(value))),
            "multipleOf" => parts.push(format!("a multiple of {value}")),
            "minimum" | "exclusiveMinimum" => parts.push(range(schema)),
            "maximum" | "exclusiveMaximum" => {
                if !schema.contains_key("minimum") && !schema.contains_key("exclusiveMinimum") {
                    parts.push(range(schema));
                }
            }
            "minLength" => parts.push(length(schema)),
            "maxLength" => {
                if !schema.contains_key("minLength") {
                    parts.push(length(schema));
                }
            }
            "minItems" => parts.push(count(schema)),
            "maxItems" => {
                if !schema.contains_key("minItems") {
                    parts.push(count(schema));
                }
            }
            "uniqueItems" => {
                if value.as_bool() == Some(true) {
                    parts.push("distinct".to_owned());
                }
            }
            "properties" => parts.push(fields(value, schema.get("required"))),
            "items" | "additionalProperties"
                if value.as_object().is_some_and(|held| !held.is_empty()) =>
            {
                let inner = describe_constraint(Some(value));
                if !inner.is_empty() {
                    // Parenthesised where the element has more than one thing to say about it, so
                    // `object, of (array, distinct)` cannot be read as a distinct object.
                    parts.push(if inner.contains(',') {
                        format!("of ({inner})")
                    } else {
                        format!("of {inner}")
                    });
                }
            }
            _ => {}
        }
    }

    for (keyword, value) in schema {
        if !CONSTRAINT_ORDER.contains(&keyword.as_str())
            && !CONSTRAINT_FOLDED.contains(&keyword.as_str())
        {
            parts.push(format!("{keyword}={value}"));
        }
    }
    parts.join(", ")
}

/// One scalar without the quotes JSON would put round it.
fn plain(value: &Json) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// The numeric bounds, folded into one phrase.
fn range(schema: &Map<String, Json>) -> String {
    let held = |name: &str| schema.get(name).map(ToString::to_string);
    let (low, low_open) = (held("minimum"), held("exclusiveMinimum"));
    let (high, high_open) = (held("maximum"), held("exclusiveMaximum"));
    let lower = low_open
        .as_ref()
        .map(|bound| format!("above {bound}"))
        .or(low);
    let upper = high_open
        .as_ref()
        .map(|bound| format!("below {bound}"))
        .or(high);

    match (lower, upper) {
        (Some(lower), Some(upper)) => {
            if low_open.is_none() && high_open.is_none() {
                format!("{lower} to {upper}")
            } else {
                format!("{lower}, {upper}")
            }
        }
        (Some(lower), None) => {
            if low_open.is_some() {
                lower
            } else {
                format!("at least {lower}")
            }
        }
        (None, Some(upper)) => {
            if high_open.is_some() {
                upper
            } else {
                format!("at most {upper}")
            }
        }
        (None, None) => String::new(),
    }
}

/// `minItems` and `maxItems`, folded the way [`length`] folds their string equivalents.
fn count(schema: &Map<String, Json>) -> String {
    match (schema.get("minItems"), schema.get("maxItems")) {
        (Some(low), Some(high)) => format!("{low} to {high} items"),
        (Some(low), None) => format!("at least {low} item(s)"),
        (None, Some(high)) => format!("at most {high} item(s)"),
        (None, None) => String::new(),
    }
}

/// A struct element's fields, in the order the producer declared them, required ones starred.
///
/// Declaration order rather than alphabetical: that is the order the struct is written in, and the
/// order somebody comparing this against the source will read it in.
fn fields(properties: &Json, required: Option<&Json>) -> String {
    let required: Vec<&str> = required
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
        .collect();
    let names: Vec<String> = properties
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| {
            if required.contains(&name.as_str()) {
                format!("{name}*")
            } else {
                name.clone()
            }
        })
        .collect();
    if names.is_empty() {
        "no fields".to_owned()
    } else {
        format!("fields {}", names.join(", "))
    }
}

/// The string-length bounds, folded into one phrase.
fn length(schema: &Map<String, Json>) -> String {
    match (schema.get("minLength"), schema.get("maxLength")) {
        (Some(low), Some(high)) => format!("{low} to {high} characters"),
        (Some(low), None) => format!("at least {low} characters"),
        (None, Some(high)) => format!("at most {high} characters"),
        (None, None) => String::new(),
    }
}

/// Re-flow a documentation block to this width, keeping its paragraph breaks.
#[must_use]
pub fn prose(text: &str, indent: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for (index, paragraph) in text.split("\n\n").enumerate() {
        let collapsed = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            continue;
        }
        if index > 0 && !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(wrap(&collapsed, WIDTH - indent.chars().count()));
    }
    lines
        .into_iter()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect()
}

/// Greedy wrap on whitespace, never breaking a word and never re-spacing one that fits.
///
/// A spelling, a pattern or an image reference is a single token, and a token split across two lines
/// is one nobody can copy — so a value wider than the width overflows rather than being broken.
///
/// Internal whitespace is carried rather than collapsed: `PathBuf  (read as text)` puts two spaces
/// between the type and its gloss on purpose, and a wrapper that normalised them would quietly
/// re-format every line it did not need to touch. Only whitespace a line break lands on is dropped.
#[must_use]
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let limit = width.max(16);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut pending = String::new();

    for chunk in chunks(text) {
        if chunk.starts_with(char::is_whitespace) {
            chunk.clone_into(&mut pending);
            continue;
        }
        let width_of = |held: &str| held.chars().count();
        if !current.is_empty() && width_of(&current) + width_of(&pending) + width_of(chunk) > limit
        {
            lines.push(std::mem::take(&mut current));
            pending.clear();
        }
        current.push_str(&std::mem::take(&mut pending));
        current.push_str(chunk);
    }
    if lines.is_empty() || !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// One text as alternating runs of whitespace and of everything else.
fn chunks(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut at = 0;
    for (index, held) in text.char_indices() {
        let boundary =
            index > at && text[at..].starts_with(char::is_whitespace) != held.is_whitespace();
        if boundary {
            found.push(&text[at..index]);
            at = index;
        }
    }
    if at < text.len() {
        found.push(&text[at..]);
    }
    found
}

/// The images that read one setting, or `all N` when that is every image the chart pins.
///
/// `all 9` rather than nine names is not an abbreviation: the nine are listed in the header, and the
/// fact worth reading here is that the setting is fleet-wide. It is also what keeps a key read by
/// eight visibly different from a loader variable read by all nine.
#[must_use]
pub fn reader_list(setting: &Setting, total: usize) -> String {
    if total > 1 && setting.occurrences.len() == total {
        return format!("all {total}");
    }
    setting.readers().join(", ")
}

/// The two-character flag column: `R` required, `S` secret.
#[must_use]
pub fn markers(entry: &Map<String, Json>) -> String {
    let held = |name: &str| entry.get(name).and_then(Json::as_bool) == Some(true);
    format!(
        "{}{}",
        if held("required") { "R" } else { "." },
        if held("secret") { "S" } else { "." }
    )
}

/// The default as the loader would read it, or `-` when the setting has none.
///
/// An empty string is a default, not an absence — `""` and "nothing is set" are two different
/// deployments — so it is spelt out rather than folded into the same dash.
#[must_use]
pub fn shown_default(entry: &Map<String, Json>) -> String {
    match entry.get("default") {
        None | Some(Json::Null) => "-".to_owned(),
        Some(Json::String(text)) if text.is_empty() => "(empty)".to_owned(),
        Some(Json::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// One value clipped to a column, with an ellipsis where it was cut.
#[must_use]
pub fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    format!("{}...", text.chars().take(width - 3).collect::<String>())
}

/// One line per setting: path, type, markers, default, and the images that read it.
///
/// The columns size themselves to the selection rather than to the chart, so a filtered listing is
/// narrow where the whole chart is wide, and the default column disappears entirely when nothing in
/// the selection has one. The reader list is last and unpadded: it is the widest field and the one
/// that varies most, and truncating it would hide the fact this listing exists to show.
#[must_use]
pub fn compact(settings: &[&Setting], total: usize) -> Vec<String> {
    let rows: Vec<[String; 5]> = settings
        .iter()
        .map(|setting| {
            let entry = setting.representative();
            let form = Entry(&entry)
                .text_form()
                .map_or_else(|_| "unknown".to_owned(), |form| form.label().to_owned());
            [
                clip(&setting.name, MAX_PATH),
                clip(
                    entry
                        .get("ty")
                        .and_then(Json::as_str)
                        .filter(|held| !held.is_empty())
                        .unwrap_or(&form),
                    MAX_TYPE,
                ),
                markers(&entry),
                clip(&shown_default(&entry), MAX_DEFAULT),
                if total > 1 {
                    reader_list(setting, total)
                } else {
                    String::new()
                },
            ]
        })
        .collect();

    let width = |column: usize| {
        rows.iter()
            .map(|row| row[column].chars().count())
            .max()
            .unwrap_or(0)
    };
    let (path, ty, default) = (width(0), width(1), width(3));
    let defaults = rows.iter().any(|row| row[3] != "-");

    rows.iter()
        .map(|row| {
            let mut line = format!(
                "    {:<path$}  {:<ty$}  {}",
                row[0],
                row[1],
                row[2],
                path = path,
                ty = ty
            );
            if defaults {
                line = format!("{line}  {:<default$}", row[3], default = default);
            }
            if row[4].is_empty() {
                line.trim_end().to_owned()
            } else {
                format!("{}  {}", line.trim_end(), row[4])
            }
        })
        .collect()
}

/// One labelled line, wrapped under its own label rather than off the terminal.
///
/// A spelling or a type never needs this; a refusal explaining why a file cannot supply the key
/// always does, and an entry whose widest line is the one carrying the reason is the one an operator
/// will not read.
fn row(into: &mut Vec<String>, label: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    let margin = format!("{:8}{label:<14}", "");
    let indent = " ".repeat(margin.chars().count());
    for (index, line) in wrap(value, WIDTH - margin.chars().count())
        .iter()
        .enumerate()
    {
        into.push(if index == 0 {
            format!("{margin}{line}")
        } else {
            format!("{indent}{line}")
        });
    }
}

/// Everything one contract says about one setting.
#[must_use]
pub fn full(setting: &Setting, dialect: &Map<String, Json>, total: usize) -> Vec<String> {
    let entry = setting.representative();
    let form = Entry(&entry)
        .text_form()
        .map_or_else(|_| "unknown".to_owned(), |form| form.label().to_owned());
    let divergent = setting.divergent();

    let mut lines = vec![format!("    {}", setting.name)];

    let ty = entry.get("ty").and_then(Json::as_str).unwrap_or("?");
    row(&mut lines, "type", &format!("{ty}  (read as {form})"));
    row(&mut lines, "required", yes_no(&entry, "required"));
    row(&mut lines, "secret", yes_no(&entry, "secret"));
    if entry.get("reserved").and_then(Json::as_bool) == Some(true) {
        row(
            &mut lines,
            "reserved",
            "yes — the loader claims this path; a deployment must not set it",
        );
    }
    row(&mut lines, "default", &shown_default(&entry));
    // Beside the default rather than at the end with the prose, because that is what a note turns
    // out to be: it glosses the default rather than the key, and a gloss printed six lines from the
    // thing it glosses is a gloss nobody connects.
    row(
        &mut lines,
        "note",
        setting
            .value("note")
            .map(plain)
            .unwrap_or_default()
            .as_str(),
    );
    if let Some(values) = entry
        .get("values")
        .and_then(Json::as_array)
        .filter(|held| !held.is_empty())
    {
        row(
            &mut lines,
            "values",
            &values.iter().map(plain).collect::<Vec<_>>().join(" | "),
        );
    }
    row(
        &mut lines,
        "accepts",
        &describe_constraint(entry.get("constraint")),
    );
    row(
        &mut lines,
        "as text",
        &describe_constraint(entry.get("text_constraint")),
    );
    if total > 1 {
        row(&mut lines, "read by", &reader_list(setting, total));
    }

    spellings(&mut lines, &entry, dialect, &form);

    if divergent.iter().any(|name| name == "docs") {
        lines.push("        docs (the images disagree)".to_owned());
        for (value, readers) in setting.variants("docs") {
            lines.push(format!("          {}:", readers.join(", ")));
            let text = value.as_str().unwrap_or("(nothing)");
            lines.extend(prose(
                if text.is_empty() { "(nothing)" } else { text },
                &" ".repeat(12),
            ));
        }
    } else if let Some(docs) = setting.value("docs").and_then(Json::as_str) {
        lines.push("        docs".to_owned());
        lines.extend(prose(docs, &" ".repeat(12)));
    }

    let remaining: Vec<&str> = divergent
        .iter()
        .map(String::as_str)
        .filter(|name| *name != "docs")
        .collect();
    if !remaining.is_empty() {
        let first = setting
            .readers()
            .first()
            .copied()
            .unwrap_or_default()
            .to_owned();
        row(
            &mut lines,
            "disagreement",
            &format!(
                "the images describe {} differently; the values above are {first}'s",
                remaining.join(", ")
            ),
        );
    }

    lines.push(String::new());
    lines
}

/// Every name the loader will accept for one key, and whether a file can supply it.
///
/// The file-supplyable reading is normative and counter-intuitive: the two file spellings exist for
/// every key, and for a key of any form but text neither of them can supply it — a file delivers a
/// string, and a loader will not coerce one into a number, a boolean or a literal. Printing the
/// spelling without printing that is how an operator ends up mounting a Secret that is never read.
fn spellings(
    lines: &mut Vec<String>,
    entry: &Map<String, Json>,
    dialect: &Map<String, Json>,
    form: &str,
) {
    let suffix = dialect
        .get("indirection_suffix")
        .and_then(Json::as_str)
        .unwrap_or("_FILE");
    for (label, spelling) in [
        ("environment".to_owned(), "env"),
        (format!("{suffix} form"), "env_file"),
        ("secrets file".to_owned(), "secrets_file"),
    ] {
        if let Some(value) = entry.get(spelling).filter(|held| !is_empty(held)) {
            row(lines, &label, &plain(value));
        }
    }
    for (label, spelling) in [
        ("env aliases", "env_aliases"),
        ("aliases", "aliases"),
        ("secrets file aliases", "secrets_file_aliases"),
    ] {
        if let Some(values) = entry
            .get(spelling)
            .and_then(Json::as_array)
            .filter(|held| !held.is_empty())
        {
            row(
                lines,
                label,
                &values.iter().map(plain).collect::<Vec<_>>().join(", "),
            );
        }
    }

    let by_file = ["env_file", "secrets_file"]
        .iter()
        .any(|spelling| entry.get(*spelling).is_some_and(|held| !is_empty(held)));
    if !by_file {
        return;
    }
    if Entry(entry).file_supplyable().unwrap_or(false) {
        row(lines, "from a file", "yes");
    } else {
        row(
            lines,
            "from a file",
            &format!(
                "no — a file supplies text, and this key is read as {form}; set it in the document \
                 or the environment"
            ),
        );
    }
}

/// One boolean field, as the entry reads.
fn yes_no(entry: &Map<String, Json>, name: &str) -> &'static str {
    if entry.get(name).and_then(Json::as_bool) == Some(true) {
        "yes"
    } else {
        "no"
    }
}

/// One loader or external variable: what it is, what it says, and who reads it.
///
/// Neither is a setting, so neither gets the full entry above — there is no path, no file spelling
/// and no secrets file to report. What they have in common is a name, a one-line summary and prose.
#[must_use]
pub fn variable(setting: &Setting, summary: &str, total: usize) -> Vec<String> {
    let mut lines = vec![format!("    {}  ({summary})", setting.name)];
    if let Some(docs) = setting
        .value("docs")
        .and_then(Json::as_str)
        .filter(|held| !held.is_empty())
    {
        lines.extend(prose(docs, &" ".repeat(8)));
    }
    if total > 1 {
        lines.push(format!("        read by  {}", reader_list(setting, total)));
    }
    lines
}

/// The whole surface, for a person.
#[must_use]
pub fn render(surface: &Surface, pattern: Option<&str>, show_full: bool) -> String {
    use std::fmt::Write as _;

    let (keys, loader, external) = (
        select(&surface.keys, pattern),
        select(&surface.loader, pattern),
        select(&surface.external, pattern),
    );
    let total = surface.readers.len();
    let dialect = |name: &str| {
        surface
            .dialect
            .get(name)
            .and_then(Json::as_str)
            .unwrap_or_default()
    };
    let prefix = dialect("prefix");

    let mut out = format!(
        "==> {}: {} setting(s) across {total} image(s)\n",
        surface.chart,
        surface.keys.len()
    );
    let _ = writeln!(
        out,
        "    {prefix}* spellings, {} for nesting, {} for a file reference",
        dialect("nesting_separator"),
        dialect("indirection_suffix")
    );
    let mut readers: Vec<&Reader> = surface.readers.iter().collect();
    readers.sort_by(|left, right| left.name.cmp(&right.name));
    let width =
        |of: fn(&Reader) -> &str| readers.iter().map(|held| of(held).len()).max().unwrap_or(0) + 2;
    let (name_width, image_width) = (width(|held| &held.name), width(|held| &held.image));
    for reader in &readers {
        let _ = writeln!(
            out,
            "    {:<name_width$}{:<image_width$}{:<10}{}...",
            reader.name,
            reader.image,
            reader.version,
            reader.digest.chars().take(19).collect::<String>()
        );
    }

    if let Some(pattern) = pattern {
        let _ = writeln!(
            out,
            "\n    matching '{pattern}': {} setting(s), {} loader variable(s), {} external \
variable(s)",
            keys.len(),
            loader.len(),
            external.len()
        );
    }

    if !keys.is_empty() {
        let _ = write!(out, "\n==> settings\n\n");
        let entries = show_full || pattern.is_some();
        if entries {
            for setting in &keys {
                let _ = writeln!(out, "{}", full(setting, &surface.dialect, total).join("\n"));
            }
        } else {
            let mut columns = "path, type, flags, default".to_owned();
            if total > 1 {
                columns.push_str(", and the images that read it");
            }
            let _ = write!(out, "    {columns}; R marks required, S marks secret\n\n");
            let _ = writeln!(out, "{}", compact(&keys, total).join("\n"));
        }
    }

    let _ = write!(
        out,
        "{}",
        variable_sections(surface, &loader, &external, total)
    );
    out
}

/// The two sections that are not settings: what locates the document, and what is nobody's.
fn variable_sections(
    surface: &Surface,
    loader: &[&Setting],
    external: &[&Setting],
    total: usize,
) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let field = |entry: &Map<String, Json>, name: &str| {
        entry
            .get(name)
            .and_then(Json::as_str)
            .unwrap_or("null")
            .to_owned()
    };

    if !loader.is_empty() {
        let _ = write!(
            out,
            "\n==> loader variables — where the settings are read from\n\n"
        );
        let blocks: Vec<String> = loader
            .iter()
            .map(|setting| {
                let entry = setting.representative();
                variable(
                    setting,
                    &format!(
                        "role {}, default {}",
                        field(&entry, "role"),
                        shown_default(&entry)
                    ),
                    total,
                )
                .join("\n")
            })
            .collect();
        let _ = writeln!(out, "{}", blocks.join("\n\n"));
    }

    if external.is_empty() {
        return out;
    }

    let _ = write!(
        out,
        "\n==> external variables — read by the image, and nobody's configuration\n\n"
    );
    let _ = writeln!(
        out,
        "{}",
        prose(
            "These are not settings. They belong to a runtime or a library the image carries, they \
             are outside the loader's namespace, and nothing in this chart's document can change \
             one. A value set here and expected to reach the application's configuration is a \
             value that is ignored.",
            "    "
        )
        .join("\n")
    );
    let _ = writeln!(out);

    let blocks: Vec<String> = external
        .iter()
        .map(|setting| {
            let entry = setting.representative();
            let constraint = describe_constraint(entry.get("constraint"));
            let summary = format!(
                "owner {}, {}{}, default {}",
                field(&entry, "owner"),
                field(&entry, "ty"),
                if constraint.is_empty() {
                    String::new()
                } else {
                    format!(", {constraint}")
                },
                shown_default(&entry)
            );
            variable(setting, &summary, total).join("\n")
        })
        .collect();
    let _ = writeln!(out, "{}", blocks.join("\n\n"));

    let prefix = surface
        .dialect
        .get("prefix")
        .and_then(Json::as_str)
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "\n    unaccounted-for variables under {prefix}*: {}",
        surface.unknown
    );
    if !surface.ignore.is_empty() {
        let _ = writeln!(out, "    ignored by pattern: {}", surface.ignore.join(", "));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use serde_json::{Map, Value as Json, json};

    use super::{
        Setting, Surface, collect, compact, describe_constraint, full, reader_list,
        report_divergences, select,
    };
    use crate::helm::declaration::load_declaration;
    use crate::report::Report;

    const API: &str = include_str!("../../tests/fixtures/api.json");
    const WORKER: &str = include_str!("../../tests/fixtures/worker.json");

    const API_DIGEST: &str =
        "sha256:abababababababababababababababababababababababababababababababab";
    const WORKER_DIGEST: &str =
        "sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
    const OTHER_DIGEST: &str =
        "sha256:efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef";

    fn fixture(name: &str) -> Json {
        serde_json::from_str(match name {
            "api" => API,
            _ => WORKER,
        })
        .expect("a fixture contract")
    }

    fn setting(name: &str, occurrences: &[(&str, Json)]) -> Setting {
        Setting {
            name: name.to_owned(),
            occurrences: occurrences
                .iter()
                .map(|(reader, entry)| {
                    (
                        (*reader).to_owned(),
                        entry.as_object().expect("an object").clone(),
                    )
                })
                .collect(),
        }
    }

    /// A throwaway chart pinning one image per contract, wired exactly as a real one is.
    struct Chart(PathBuf);

    impl Chart {
        /// `images` is `(document name, fixture name, digest)`, one document per image.
        fn of(images: &[(&str, &str, &str)]) -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let at = std::env::temp_dir()
                .join(format!(
                    "terrace-explain-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ))
                .join("demo");
            let _ = std::fs::remove_dir_all(&at);
            std::fs::create_dir_all(at.join("contracts")).expect("a chart directory");

            let mut values = Map::new();
            let mut declared = String::new();
            for (name, contract, digest) in images {
                use std::fmt::Write as _;

                std::fs::write(
                    at.join("contracts").join(format!("{name}.json")),
                    serde_json::to_string(&json!({
                        "source": {
                            "image": format!("docker.io/demo/{name}"),
                            "digest": digest,
                            "sha256": "0".repeat(64),
                            "fetched": "2026-01-01T00:00:00Z",
                        },
                        "contract": fixture(contract),
                    }))
                    .expect("it serialises"),
                )
                .expect("a vendored contract is written");
                values.insert(
                    (*name).to_owned(),
                    json!({"repository": format!("demo/{name}"), "tag": format!("v1@{digest}")}),
                );
                let _ = write!(
                    declared,
                    "  - name: {name}\n    source: {{ kind: ConfigMap, key: config.toml }}\n    \
                     images:\n      - values: {name}\n        contract: contracts/{name}.json\n"
                );
            }

            std::fs::write(at.join("Chart.yaml"), "name: demo\nappVersion: v1\n")
                .expect("a chart file");
            std::fs::write(
                at.join("values.yaml"),
                serde_json::to_string(&Json::Object(values)).expect("it serialises"),
            )
            .expect("a values file");
            std::fs::write(
                at.join("config-contract.yaml"),
                format!("documents:\n{declared}"),
            )
            .expect("a declaration");
            Self(at)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }

        /// Repin one image at another digest, which is what the interlock exists to catch.
        fn repin(&self, name: &str, digest: &str) {
            let text =
                std::fs::read_to_string(self.0.join("values.yaml")).expect("the values are there");
            let mut values: Json = serde_json::from_str(&text).expect("they parse");
            values[name]["tag"] = json!(format!("v2@{digest}"));
            std::fs::write(
                self.0.join("values.yaml"),
                serde_json::to_string(&values).expect("it serialises"),
            )
            .expect("the values are written");
        }

        fn collected(&self) -> (Option<Surface>, Report) {
            let mut report = Report::new();
            let declaration = load_declaration(self.path())
                .expect("the declaration reads")
                .expect("it is there");
            let surface = collect(self.path(), &declaration, &mut report).expect("it collects");
            (surface, report)
        }
    }

    impl Drop for Chart {
        fn drop(&mut self) {
            if let Some(parent) = self.0.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }
    }

    // -- which images read which setting -----------------------------------------------------

    fn two_images() -> Chart {
        Chart::of(&[
            ("api", "api", API_DIGEST),
            ("worker", "worker", WORKER_DIGEST),
        ])
    }

    #[test]
    fn a_setting_names_exactly_the_images_that_read_it() {
        // The one fact this produces that exists nowhere else: not in the chart, not in the
        // declaration, not in the README.
        let chart = two_images();
        let (surface, _) = chart.collected();
        let surface = surface.expect("the contracts bind");
        assert_eq!(surface.keys["auth.session_ttl"].readers(), ["api"]);
        assert_eq!(surface.keys["worker.concurrency"].readers(), ["worker"]);
        assert_eq!(surface.keys["database.url"].readers(), ["api", "worker"]);
        assert_eq!(
            surface.keys.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "auth.session_ttl",
                "database.url",
                "github.repos",
                "log.level",
                "tuning.ratio",
                "worker.concurrency",
                "worker.debug",
            ]
        );
    }

    #[test]
    fn the_images_are_named_by_their_contract_and_carry_their_provenance() {
        let chart = two_images();
        let (surface, _) = chart.collected();
        let surface = surface.expect("the contracts bind");
        let api = surface
            .readers
            .iter()
            .find(|reader| reader.name == "api")
            .expect("the api image");
        assert_eq!(api.image, "docker.io/demo/api");
        assert_eq!(api.digest, API_DIGEST);
        assert_eq!(api.documents, ["api"]);
    }

    #[test]
    fn readers_are_sorted_rather_than_declaration_ordered() {
        // The order every reader list and every "the images disagree" side is printed in is a
        // property of the contracts rather than of where a maintainer added a document.
        let chart = Chart::of(&[
            ("worker", "worker", WORKER_DIGEST),
            ("api", "api", API_DIGEST),
        ]);
        let (surface, _) = chart.collected();
        assert_eq!(
            surface.expect("the contracts bind").keys["database.url"].readers(),
            ["api", "worker"]
        );
    }

    #[test]
    fn all_of_them_is_said_as_all_rather_than_listed() {
        // The names are in the header; what is worth reading here is that the setting is
        // fleet-wide. One image is not a fleet, and `all 1` would be a strange way to say a name.
        let chart = two_images();
        let (surface, _) = chart.collected();
        let surface = surface.expect("the contracts bind");
        let shared = &surface.keys["database.url"];
        assert_eq!(reader_list(shared, 2), "all 2");
        assert_eq!(reader_list(shared, 3), "api, worker");
        assert_eq!(reader_list(&surface.keys["log.level"], 1), "api");
    }

    // -- merging what several images say -----------------------------------------------------

    #[test]
    fn required_unions_because_any_reader_requiring_it_wins() {
        let merged = setting(
            "database.url",
            &[
                ("api", json!({"path": "database.url", "required": false})),
                ("worker", json!({"path": "database.url", "required": true})),
            ],
        );
        assert_eq!(merged.value("required"), Some(&json!(true)));
        // The rule restated rather than reinvented, so a union is not reported as a disagreement.
        assert!(!merged.divergent().iter().any(|name| name == "required"));
    }

    #[test]
    fn empty_prose_is_an_absence_rather_than_a_contradiction() {
        // An image that says nothing about a key it shares has not contradicted the one that does.
        let merged = setting(
            "bind_addr",
            &[
                ("api", json!({"path": "bind_addr", "docs": ""})),
                (
                    "worker",
                    json!({"path": "bind_addr", "docs": "The listener."}),
                ),
            ],
        );
        assert_eq!(merged.value("docs"), Some(&json!("The listener.")));
    }

    #[test]
    fn two_descriptions_of_one_setting_are_both_kept() {
        let merged = setting(
            "bind_addr",
            &[
                ("api", json!({"path": "bind_addr", "docs": "Public."})),
                ("render", json!({"path": "bind_addr", "docs": ""})),
                ("worker", json!({"path": "bind_addr", "docs": "Ops only."})),
            ],
        );
        assert_eq!(merged.divergent(), ["docs"]);
        assert_eq!(
            merged.variants("docs"),
            [
                (json!("Public."), vec!["api"]),
                (json!(""), vec!["render"]),
                (json!("Ops only."), vec!["worker"]),
            ]
        );
    }

    #[test]
    fn a_disagreement_about_a_constraint_is_reported_and_not_resolved() {
        let merged = setting(
            "port",
            &[
                (
                    "api",
                    json!({"path": "port", "constraint": {"type": "integer", "maximum": 65535}}),
                ),
                (
                    "worker",
                    json!({"path": "port", "constraint": {"type": "integer"}}),
                ),
            ],
        );
        assert_eq!(merged.divergent(), ["constraint"]);
        assert_eq!(
            merged.representative()["constraint"]["maximum"],
            json!(65535)
        );
    }

    #[test]
    fn a_field_only_one_image_publishes_is_still_carried() {
        let merged = setting(
            "log.level",
            &[
                ("api", json!({"path": "log.level"})),
                (
                    "worker",
                    json!({"path": "log.level", "note": "ignored in tests"}),
                ),
            ],
        );
        assert_eq!(merged.value("note"), Some(&json!("ignored in tests")));
    }

    #[test]
    fn the_representative_carries_every_field_any_image_published() {
        // Including one only the second image says anything about, and in the document model's own
        // order — which is the order this build's producer writes a contract in, so the JSON diffs
        // cleanly against the file it came from.
        let merged = setting(
            "log.level",
            &[
                (
                    "api",
                    json!({"path": "log.level", "env": "X", "ty": "Level"}),
                ),
                (
                    "worker",
                    json!({"path": "log.level", "env": "X", "ty": "Level", "secret": false}),
                ),
            ],
        );
        assert_eq!(
            merged
                .representative()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["env", "path", "secret", "ty"]
        );
    }

    // -- the divergence nothing else can see -------------------------------------------------

    #[test]
    fn a_shared_setting_described_differently_becomes_a_warning() {
        // Each document is validated against its own images' union, so two images that never share
        // a document never have their descriptions of a shared setting compared.
        let chart = two_images();
        let (surface, mut report) = chart.collected();
        let mut surface = surface.expect("the contracts bind");
        surface
            .keys
            .get_mut("database.url")
            .expect("the key")
            .occurrences[1]
            .1
            .insert("docs".to_owned(), json!("Somewhere else entirely."));

        report_divergences(&surface, None, &mut report);
        let messages: Vec<&str> = report
            .entries()
            .iter()
            .map(|entry| entry.finding.message.as_str())
            .collect();
        assert!(
            messages.iter().any(|held| held.contains("database.url")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|held| held.contains("`docs`")),
            "{messages:?}"
        );
        assert_eq!(report.error_count(), 0);
    }

    #[test]
    fn a_divergence_outside_the_selection_is_not_reported() {
        // A warning about a setting the reader did not ask about is noise on every run, and would
        // train them to ignore the one that matters.
        let chart = two_images();
        let (surface, mut report) = chart.collected();
        let mut surface = surface.expect("the contracts bind");
        surface
            .keys
            .get_mut("database.url")
            .expect("the key")
            .occurrences[1]
            .1
            .insert("docs".to_owned(), json!("drifted"));
        report_divergences(&surface, Some("log.level"), &mut report);
        assert_eq!(report.entries().len(), 0);
    }

    // -- the interlock -----------------------------------------------------------------------

    #[test]
    fn a_bumped_digest_produces_no_surface_at_all() {
        // Printing a contract that is not for the pinned digest is a confident wrong answer to the
        // only question anyone runs this to ask.
        let chart = Chart::of(&[("api", "api", API_DIGEST)]);
        chart.repin("api", OTHER_DIGEST);
        let (surface, report) = chart.collected();
        assert!(surface.is_none());
        assert!(report.error_count() > 0);
    }

    #[test]
    fn one_stale_document_refuses_the_whole_chart() {
        // Rather than printing the one that bound and quietly dropping the other: a listing missing
        // a document is indistinguishable from an image that reads nothing.
        let chart = two_images();
        chart.repin("worker", OTHER_DIGEST);
        assert!(chart.collected().0.is_none());
    }

    // -- selecting ---------------------------------------------------------------------------

    fn settings() -> BTreeMap<String, Setting> {
        [
            "auth.session_ttl",
            "database.url",
            "log.level",
            "Log.Rotation",
        ]
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                setting(name, &[("api", json!({"path": name}))]),
            )
        })
        .collect()
    }

    fn selected(pattern: Option<&str>) -> Vec<String> {
        select(&settings(), pattern)
            .into_iter()
            .map(|item| item.name.clone())
            .collect()
    }

    #[test]
    fn no_pattern_takes_everything_in_path_order() {
        assert_eq!(
            selected(None),
            [
                "Log.Rotation",
                "auth.session_ttl",
                "database.url",
                "log.level"
            ]
        );
    }

    #[test]
    fn a_plain_pattern_is_a_case_insensitive_substring() {
        // The name an operator has in hand is usually a fragment of a path they read somewhere.
        assert_eq!(selected(Some("LOG")), ["Log.Rotation", "log.level"]);
    }

    #[test]
    fn a_glob_is_matched_against_the_whole_path() {
        assert_eq!(selected(Some("log.*")), ["log.level"]);
        assert!(selected(Some("auth.*.ttl")).is_empty());
        assert_eq!(selected(Some("log.leve?")), ["log.level"]);
        assert_eq!(selected(Some("[dl]og.level")), ["log.level"]);
        assert_eq!(selected(Some("[!L]og.level")), ["log.level"]);
    }

    // -- rendering a constraint --------------------------------------------------------------

    fn described(constraint: &Json) -> String {
        describe_constraint(Some(constraint))
    }

    #[test]
    fn a_bound_reads_as_a_range_and_says_which_side_when_it_is_one_sided() {
        assert_eq!(
            described(&json!({"type": "integer", "minimum": 0, "maximum": 65535})),
            "integer, 0 to 65535"
        );
        assert_eq!(
            described(&json!({"type": "integer", "minimum": 1})),
            "integer, at least 1"
        );
        assert_eq!(
            described(&json!({"type": "integer", "maximum": 9})),
            "integer, at most 9"
        );
        assert_eq!(described(&json!({"exclusiveMinimum": 0})), "above 0");
    }

    #[test]
    fn a_vocabulary_this_renderer_knows_reads_as_prose() {
        assert_eq!(
            described(&json!({"type": "string", "enum": ["a", "b"]})),
            "string, one of \"a\" | \"b\""
        );
        assert_eq!(
            described(&json!({"type": "string", "minLength": 1, "maxLength": 64})),
            "string, 1 to 64 characters"
        );
        assert_eq!(
            described(&json!({"type": "array", "items": {"type": "string"}, "uniqueItems": true})),
            "array, distinct, of string"
        );
    }

    #[test]
    fn an_unrecognised_keyword_is_printed_rather_than_dropped() {
        // Under-reporting a constraint is the failure this pipeline exists to remove, and it is no
        // better coming from the explainer than from a gate.
        assert!(
            described(&json!({"type": "string", "contentEncoding": "base64"}))
                .contains("contentEncoding=\"base64\""),
            "{}",
            described(&json!({"type": "string", "contentEncoding": "base64"}))
        );
    }

    #[test]
    fn nothing_to_say_is_said_with_nothing() {
        assert_eq!(describe_constraint(None), "");
        assert_eq!(described(&json!({})), "");
    }

    // -- rendering an entry ------------------------------------------------------------------

    fn dialect() -> Map<String, Json> {
        json!({
            "prefix": "FIXTURE_",
            "nesting_separator": "__",
            "indirection_suffix": "_FILE",
        })
        .as_object()
        .expect("an object")
        .clone()
    }

    fn key(path: &str) -> Setting {
        let contract = fixture("api");
        let entry = contract["schema"]["keys"]
            .as_array()
            .expect("a list of keys")
            .iter()
            .find(|held| held["path"] == json!(path))
            .unwrap_or_else(|| panic!("{path}"));
        setting(path, &[("api", entry.clone())])
    }

    fn rendered(path: &str, total: usize) -> String {
        full(&key(path), &dialect(), total).join("\n")
    }

    #[test]
    fn every_spelling_the_loader_accepts_is_shown() {
        let text = rendered("database.url", 1);
        assert!(text.contains("FIXTURE_DATABASE__URL"), "{text}");
        assert!(text.contains("FIXTURE_DATABASE__URL_FILE"), "{text}");
        assert!(text.contains("database__url"), "{text}");
        assert!(text.contains("from a file   yes"), "{text}");
    }

    #[test]
    fn a_key_no_file_can_supply_says_so_beside_the_spellings() {
        // The file spellings exist for every key, and for anything but a text key neither works —
        // a file delivers a string and a loader will not coerce one. Printing the spelling without
        // printing that is how an operator ends up mounting a Secret that is silently never read.
        let text = rendered("auth.session_ttl", 1);
        assert!(text.contains("FIXTURE_AUTH__SESSION_TTL_FILE"), "{text}");
        assert!(text.contains("from a file   no"), "{text}");
        assert!(text.contains("read as integer"), "{text}");
    }

    #[test]
    fn the_markers_and_the_enum_reach_the_entry() {
        let text = rendered("database.url", 1);
        assert!(text.contains("secret        yes"), "{text}");
        assert!(text.contains("required      yes"), "{text}");
        assert!(
            rendered("log.level", 1).contains("debug | info | warn"),
            "{}",
            rendered("log.level", 1)
        );
    }

    #[test]
    fn a_single_image_chart_does_not_attribute_readers() {
        assert!(!rendered("database.url", 1).contains("read by"));
        assert!(rendered("database.url", 2).contains("read by"));
    }

    // -- the compact listing -----------------------------------------------------------------

    fn rows(paths: &[&str], total: usize) -> Vec<String> {
        let settings: Vec<Setting> = paths.iter().map(|path| key(path)).collect();
        let held: Vec<&Setting> = settings.iter().collect();
        compact(&held, total)
    }

    #[test]
    fn required_and_secret_are_two_markers_in_one_column() {
        assert!(rows(&["database.url"], 1)[0].contains("RS"));
        assert!(rows(&["log.level"], 1)[0].contains(".."));
    }

    #[test]
    fn the_default_column_disappears_when_nothing_has_one() {
        // A producer that publishes no defaults leaves 167 rows of a column holding only dashes,
        // which costs width and says nothing.
        assert!(!rows(&["log.level"], 1)[0].contains('-'));
        assert!(rows(&["auth.session_ttl"], 1)[0].contains("3600"));
    }
}
