//! Reading a chart's vendored contracts back out, and saying what its images actually consume.
//!
//! Every other consumer of a chart's contracts is a gate: it takes a rendered manifest, holds it
//! against the contract and reports the difference. Nothing reads the contract for its own sake, so
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
//! nowhere which of them reads a given setting: not in the chart, not in the declaration, not in
//! the README. It is also the fact this exists to produce: a key read by three services and another
//! by all nine is what tells an operator whether a change is local or fleet-wide. So the
//! attribution is computed by walking the contracts the declaration binds, and a hand-written map
//! is exactly the thing that would be wrong within one release.
//!
//! **The merge here is tolerant where [`crate::union`] is strict, and that is not a relaxation of
//! the rule.** A union refuses two contracts that describe one key differently, because its output
//! validates a document all of them read, and an unreconcilable pair there means the gate has
//! nothing trustworthy to say. It cannot be reused across a chart's *documents*: nine separate
//! files legitimately carry nine different titles, and merging them is not a question anyone asked.
//! What this wants is the opposite posture: a disagreement between two images about one setting is
//! a finding, not a reason to print nothing. So the merge below keeps every image's description,
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

mod constraint;
mod render;
mod text;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value as Json};

use crate::error::Error;
use crate::report::{Report, warning};
use crate::union::UNKNOWN_POLICIES;

use super::declaration::{Declaration, bind, load_vendored};

pub use self::constraint::describe_constraint;
pub use self::render::{compact, full, markers, reader_list, render, shown_default, variable};
pub use self::text::{clip, prose, wrap};

/// Width the prose is re-wrapped to.
///
/// Documentation arrives hard-wrapped at the producer's own width, which is not this one, so
/// paragraphs are re-flowed rather than printed as they came.
pub const WIDTH: usize = 96;

/// Fields whose representative value is the first *non-empty* one across the images that read a
/// setting.
///
/// An image that says nothing about a key it shares has not contradicted the one that does.
const PROSE_FIELDS: [&str; 2] = ["docs", "note"];

/// Fields a union merges rather than compares, so a difference in one is not a divergence.
const UNIONED_FIELDS: [&str; 1] = ["required"];

/// One image a chart pins, named as the declaration names it.
///
/// The name is the vendored contract's stem rather than the repository part of the image reference.
/// That is what the declaration writes and what a maintainer edits, it is short enough to list nine
/// of on one line, and it maps one-to-one to an image, where a full repository path does not fit
/// and repeats the chart name once per service. The full reference is in the header and in the
/// JSON.
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
    /// Its `schema.reload`, as published: whether it applies a change without restarting.
    /// [`None`] when it declares nothing, which is read as no reload.
    pub reload: Option<Json>,
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
    /// Its fields come out in the document model's order, which is alphabetical: the same property
    /// that makes a document this build publishes a byte-for-byte round trip through its own
    /// reader. The implementation this was ported from kept the order the producer happened to
    /// write, to the same end: that the JSON here diffs cleanly against the vendored file. Against
    /// a document written by that older producer the two orders differ; against one written by this
    /// build they are the same order, and this one is the same on every machine.
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

/// Binds every declared document, then reads the contracts binding proved belong to it.
///
/// The binding is called for its interlock and for nothing else. It answers whether the vendored
/// file is for the digest the chart pins, which is the only question that decides whether anything
/// printed below describes the deployed image, and it is answered here exactly as the gates answer
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

/// Folds one image's contract into the chart-wide surface.
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
        reload: contract
            .get("schema")
            .and_then(|schema| schema.get("reload"))
            .filter(|held| !held.is_null())
            .cloned(),
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
        // surprising (nine separate files could legitimately use nine prefixes), so it is said
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

/// Records one published entry against the name it is keyed on.
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
/// fragment of a path they read in a values file. Both are anchored on the name alone: the
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

#[cfg(test)]
mod tests;
