//! The chart lines a contract change leaves behind, which no generator edits.
//!
//! A contract refresh turns an image release into chart values, and it does that for exactly one
//! kind of change: a key the chart does not yet surface. The writers here add a value, rewrite the
//! `@schema` block of one that already exists, and regenerate everything derived from both. None of
//! them touches a template, a hand-written test suite or a CI values file — deliberately, because an
//! edit made blind to a template's `with` blocks and `if` gates is one nobody can review.
//!
//! So a change that *takes something away* reaches none of them:
//!
//! | Change | What is left behind |
//! |---|---|
//! | a key removed | `legal.dir` — the template still renders `dir:`, and the service refuses to boot on it |
//! | an element property removed | `legal.documents.*.sources` — the regenerated schema is closed, so every test and CI file that sets it fails the render, and the template still reads it |
//!
//! Measured on the `tankovault` 11 bump: the values were written and the schema regenerated
//! correctly, and then the run died on six schema refusals in `tests/legal_test.yaml`, with nothing
//! saying that the chart's own legal surface was the thing still on the old contract. This is the
//! sentence that was missing.
//!
//! # What it reads
//!
//! The comparison [`super::diff`] already makes, called directly rather than through its JSON, and
//! the chart's markers **as `values.yaml` stood at the compared revision**: the value a removed key
//! was bound to has usually been deleted by the time anyone looks, and the marker on it is the only
//! record of which chart value fed the key. Then each line under `templates/`, the hand-written
//! `tests/*.yaml` and `ci/*.yaml` that still names what went away. The generated round-trip suites
//! are skipped, because regenerating them from the contract is what repairs them.
//!
//! # Deliberately textual
//!
//! Nothing here parses a Go template, and nothing could without becoming a second, partial Helm. A
//! line-based search is the only reader that cannot silently disagree with itself. A hit is a place
//! to look, not a proof the line is wrong; no hit for a removed property is not a proof of safety
//! either, which is why every finding is printed even when nothing references it.
//!
//! The patterns are the ones the implementation this was ported from used, character for
//! character, because each was narrowed against a real chart and the narrowing is the knowledge.
//! The `regex` crate has no lookaround, so the two boundary assertions those patterns carry — "not
//! preceded by" and "not followed by" a word character or a hyphen — are applied by hand to each
//! match the engine finds. That is exact, not approximate, for every pattern built here, because
//! each one has a single possible end for a given start; the private `Search` type spells out why.
//!
//! Lines are counted as `str::lines` counts them, on `\n` with an optional `\r`, which is how an
//! editor and `grep -n` count them too.

use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value as Json;

use crate::diff::{Area, Change, Kind};
use crate::error::Error;

use super::diff::{ChartDiff, Revision, spec_of};
use super::markers::{Marker, read as read_markers};

/// One line that still names something the contract took away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The file, as the chart tree was named to this run.
    pub path: PathBuf,
    /// The line, counted from 1.
    pub line: usize,
    /// The line itself, trimmed.
    pub text: String,
}

/// One contract change the chart has to follow by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    /// The chart.
    pub chart: String,
    /// The contract key the change is to.
    pub key: String,
    /// What happened to it, as a report says it: `key removed`, or `element property 'x' removed`.
    pub what: String,
    /// The chart value a marker bound the key to at the compared revision, if any did.
    ///
    /// Without one there is nothing to search for, and the migration is reported with no
    /// references: the contract took something away that the chart never surfaced.
    pub values_path: Option<String>,
    /// Every line that still uses it. Empty is the migrated state.
    pub references: Vec<Reference>,
}

impl Migration {
    /// `chart: key: what (chart value path)`, the first line of every finding.
    pub fn headline(&self) -> String {
        let bound = self
            .values_path
            .as_ref()
            .map_or_else(String::new, |path| format!(" (chart value {path})"));
        format!("{}: {}: {}{bound}", self.chart, self.key, self.what)
    }
}

/// One step of a walk through a JSON Schema: the vocabulary a removed property's position is in.
///
/// Typed rather than a list of keyword strings, so that a property that happens to be *called*
/// `items` or `properties` is a name and not a keyword. The implementation this was ported from
/// walked a flat tuple of strings and could not tell the two apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// `properties.<name>`.
    Property(String),
    /// `items`: one element of an array.
    Items,
    /// `additionalProperties`: one value of a map whose keys the operator names.
    AdditionalProperties,
}

/// Every `properties` entry `old` declares at a position `new` no longer does.
///
/// Only positions both documents describe are compared below the removal, so a property whose whole
/// parent vanished is reported once, at the parent. A position always ends in the
/// [`Step::Property`] that was removed.
pub fn removed_properties(old: Option<&Json>, new: Option<&Json>) -> Vec<Vec<Step>> {
    let mut found = Vec::new();
    walk_removed(old, new, &mut Vec::new(), &mut found);
    found
}

fn walk_removed(
    old: Option<&Json>,
    new: Option<&Json>,
    prefix: &mut Vec<Step>,
    found: &mut Vec<Vec<Step>>,
) {
    let (Some(Json::Object(old)), Some(Json::Object(new))) = (old, new) else {
        return;
    };
    // A `serde_json::Map` here is ordered by key, which is the order the report lists them in.
    let old_properties = old.get("properties").and_then(Json::as_object);
    let new_properties = new.get("properties").and_then(Json::as_object);
    for (name, was) in old_properties.into_iter().flatten() {
        prefix.push(Step::Property(name.clone()));
        match new_properties.and_then(|held| held.get(name)) {
            None => found.push(prefix.clone()),
            Some(is) => walk_removed(Some(was), Some(is), prefix, found),
        }
        prefix.pop();
    }
    for (keyword, step) in [
        ("items", Step::Items),
        ("additionalProperties", Step::AdditionalProperties),
    ] {
        prefix.push(step);
        walk_removed(old.get(keyword), new.get(keyword), prefix, found);
        prefix.pop();
    }
}

/// Every hand migration the compared contracts call for, chart by chart.
///
/// `diffed` is what [`super::diff::collect`] found against `revision`; the markers are read from
/// the same revision. A chart the working tree no longer holds is skipped, because nothing in it
/// can still reference anything.
///
/// # Errors
/// [`Error::Invalid`] when a chart's `values.yaml` at the revision carries markers that cannot be
/// read as ones, and [`Error::Io`] when a file that has to be searched cannot be read.
pub fn migrations(
    charts: &Path,
    diffed: &[ChartDiff],
    revision: &dyn Revision,
) -> Result<Vec<Migration>, Error> {
    let mut found = Vec::new();
    for chart in diffed {
        let chart_dir = charts.join(&chart.chart);
        if !chart_dir.is_dir() {
            continue;
        }
        let changes: Vec<&Change> = chart
            .contracts
            .iter()
            .flat_map(|contract| contract.changes.iter())
            .collect();
        found.extend(migrations_for(
            charts,
            &chart_dir,
            &chart.chart,
            &changes,
            revision,
        )?);
    }
    Ok(found)
}

/// Whether one change is a removal a chart could still be using.
///
/// A removed key, and a changed `constraint` — the field an element's properties live in. Every
/// other change either adds something, which the writers follow, or moves something no chart line
/// spells.
fn is_removal(change: &Change) -> bool {
    change.area == Area::Key
        && (change.kind == Kind::Removed
            || (change.kind == Kind::Changed && change.field.as_deref() == Some("constraint")))
}

/// The hand migrations one chart's changes call for.
fn migrations_for(
    charts: &Path,
    chart_dir: &Path,
    chart: &str,
    changes: &[&Change],
    revision: &dyn Revision,
) -> Result<Vec<Migration>, Error> {
    let relevant: Vec<&Change> = changes
        .iter()
        .copied()
        .filter(|change| is_removal(change))
        .collect();
    if relevant.is_empty() {
        return Ok(Vec::new());
    }

    let markers = markers_at(charts, chart_dir, chart, revision)?;
    // Read only when something is bound, so a chart whose removals reached no chart value is never
    // searched — and a file that could not be read does not fail a report that never needed it.
    let bound = relevant
        .iter()
        .any(|change| value_for(&markers, &change.subject).is_some());
    let files = if bound {
        Files::of(chart_dir)?
    } else {
        Files(Vec::new())
    };
    let mut found = Vec::new();

    for change in relevant {
        let key = change.subject.clone();
        let values_path = value_for(&markers, &key);

        if change.kind == Kind::Removed {
            let references = match &values_path {
                Some(path) => key_references(&files, path),
                None => Vec::new(),
            };
            found.push(Migration {
                chart: chart.to_owned(),
                key,
                what: "key removed".to_owned(),
                values_path,
                references,
            });
            continue;
        }

        // A `constraint` change carries the two constraints themselves, not the two entries.
        for position in removed_properties(change.old.as_ref(), change.new.as_ref()) {
            let Some(Step::Property(name)) = position.last() else {
                continue;
            };
            let references = match &values_path {
                Some(path) => property_references(&files, path, &position, name),
                None => Vec::new(),
            };
            found.push(Migration {
                chart: chart.to_owned(),
                key: key.clone(),
                what: format!("element property {} removed", crate::gate::quoted(name)),
                values_path: values_path.clone(),
                references,
            });
        }
    }
    Ok(found)
}

/// The chart's markers as `values.yaml` stood at the revision; empty when it did not exist there.
fn markers_at(
    charts: &Path,
    chart_dir: &Path,
    chart: &str,
    revision: &dyn Revision,
) -> Result<Vec<Marker>, Error> {
    let Some(text) = revision.read(&spec_of(&chart_dir.join("values.yaml"), charts)) else {
        return Ok(Vec::new());
    };
    // The line a marker error names is a line of the file at the revision, not of the working
    // tree, and a reader comparing it against their editor has to be told which.
    let (markers, _) = read_markers(&text, chart).map_err(|failure| {
        Error::Invalid(format!("{failure} (as it stood at {})", revision.name()))
    })?;
    Ok(markers)
}

/// The chart value bound to contract key `key`, if a marker named it.
fn value_for(markers: &[Marker], key: &str) -> Option<String> {
    markers
        .iter()
        .find(|marker| marker.target == key)
        .map(|marker| marker.values_path.clone())
}

// ------------------------------------------------------------------------------------------
// The files a change can leave behind
// ------------------------------------------------------------------------------------------

/// Which hand-written part of a chart a file is, which decides how it spells a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// Under `templates/`, at any depth.
    Template,
    /// A hand-written helm-unittest suite, `tests/*.yaml`.
    Suite,
    /// A CI values file, `ci/*.yaml`.
    CiValues,
}

/// One candidate file, read once however many changes it is searched for.
#[derive(Debug)]
struct File {
    path: PathBuf,
    origin: Origin,
    text: String,
}

/// The hand-written files a contract change can leave behind, in report order.
#[derive(Debug)]
struct Files(Vec<File>);

impl Files {
    /// `templates/**`, then the hand-written `tests/*.yaml`, then `ci/*.yaml`, each sorted.
    ///
    /// Sorted by path *component*, which is what [`PathBuf`]'s ordering already is, so a file in
    /// `templates/a/` sorts before `templates/a-b.yaml` exactly as the report this was ported from
    /// listed them.
    fn of(chart_dir: &Path) -> Result<Self, Error> {
        let mut templates = Vec::new();
        collect_tree(&chart_dir.join("templates"), &mut templates)?;
        templates.sort();

        let mut suites = yaml_files(&chart_dir.join("tests"))?;
        suites.retain(|path| !is_generated_suite(path));
        let ci = yaml_files(&chart_dir.join("ci"))?;

        let mut files = Vec::new();
        for (paths, origin) in [
            (templates, Origin::Template),
            (suites, Origin::Suite),
            (ci, Origin::CiValues),
        ] {
            for path in paths {
                let text =
                    std::fs::read_to_string(&path).map_err(|e| Error::io(path.display(), e))?;
                files.push(File { path, origin, text });
            }
        }
        Ok(Self(files))
    }
}

/// Every file under `directory`, at any depth, without following a symbolic link to a directory.
///
/// A missing directory holds nothing. A link to a *file* is followed, because the file it names is
/// one the chart ships.
fn collect_tree(directory: &Path, found: &mut Vec<PathBuf>) -> Result<(), Error> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory).map_err(|e| Error::io(directory.display(), e))? {
        let entry = entry.map_err(|e| Error::io(directory.display(), e))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|e| Error::io(path.display(), e))?;
        if kind.is_dir() {
            collect_tree(&path, found)?;
        } else if path.is_file() {
            found.push(path);
        }
    }
    Ok(())
}

/// The `*.yaml` files directly in `directory`, sorted; empty when it is not there.
fn yaml_files(directory: &Path) -> Result<Vec<PathBuf>, Error> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in std::fs::read_dir(directory).map_err(|e| Error::io(directory.display(), e))? {
        let path = entry.map_err(|e| Error::io(directory.display(), e))?.path();
        let named_yaml = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".yaml"));
        if named_yaml && path.is_file() {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

/// Whether a suite is one `tests` generates from the contract: `contract_roundtrip_<…>_test.yaml`.
///
/// A stale reference in one is repaired by regenerating it, not by hand.
fn is_generated_suite(path: &Path) -> bool {
    const PREFIX: &str = "contract_roundtrip_";
    const SUFFIX: &str = "_test.yaml";
    let Some(name) = path.file_name().map(|name| name.to_string_lossy()) else {
        return false;
    };
    name.len() >= PREFIX.len() + SUFFIX.len()
        && name.starts_with(PREFIX)
        && name.ends_with(SUFFIX)
        && !name[PREFIX.len()..name.len() - SUFFIX.len()].contains('\n')
}

// ------------------------------------------------------------------------------------------
// The searches
// ------------------------------------------------------------------------------------------

/// One textual search, with the two boundary assertions the `regex` crate leaves out applied by
/// hand.
///
/// `isolated_before` stands for `(?<![\w-])` in front of the pattern and `isolated_after` for
/// `(?![\w-])` behind it. Checking them on a match the engine found is exact only when the match
/// starting at a given position has one possible end — otherwise a backtracking engine could have
/// tried a shorter or longer one that satisfied the assertion. Every pattern built here has that
/// property: each ends in an escaped literal, and each variable part (`[^.\s:"']+`, `\w+`, an
/// optional `[N]`) is followed by a character it cannot itself contain. So a candidate that fails
/// an assertion is rejected and the search resumes one character later, which is precisely the set
/// of positions the original pattern would have tried.
#[derive(Debug)]
struct Search {
    pattern: Regex,
    isolated_before: bool,
    isolated_after: bool,
}

impl Search {
    /// A pattern with no assertions around it.
    fn plain(pattern: &str) -> Self {
        Self::bounded(pattern, false, false)
    }

    fn bounded(pattern: &str, isolated_before: bool, isolated_after: bool) -> Self {
        Self {
            pattern: Regex::new(pattern).expect("a pattern built from escaped text compiles"),
            isolated_before,
            isolated_after,
        }
    }

    /// Whether `text` contains a match whose surroundings satisfy both assertions.
    fn found_in(&self, text: &str) -> bool {
        if !self.isolated_before && !self.isolated_after {
            return self.pattern.is_match(text);
        }
        let mut from = 0;
        while let Some(candidate) = self.pattern.find_at(text, from) {
            let before = text[..candidate.start()].chars().next_back();
            let after = text[candidate.end()..].chars().next();
            if (!self.isolated_before || isolates(before))
                && (!self.isolated_after || isolates(after))
            {
                return true;
            }
            // One character on, not past the candidate: a rejected candidate can overlap one that
            // would have been accepted.
            let Some(next) = text[candidate.start()..].chars().next() else {
                return false;
            };
            from = candidate.start() + next.len_utf8();
        }
        false
    }
}

/// Whether a neighbouring character ends a name, which is what `[\w-]` failing to match means.
///
/// `\w` is a letter, a digit or `_`; `-` is added because chart values and Kubernetes names both
/// use it inside one name.
fn isolates(neighbour: Option<char>) -> bool {
    neighbour.is_none_or(|held| !(held.is_alphanumeric() || held == '_' || held == '-'))
}

/// Any of several searches, which is how one pattern with an alternation is held here.
#[derive(Debug)]
struct AnyOf(Vec<Search>);

impl AnyOf {
    fn found_in(&self, text: &str) -> bool {
        self.0.iter().any(|search| search.found_in(text))
    }
}

/// The dotted `--set` spelling of a schema position under a chart value.
///
/// `legal.documents` and `additionalProperties.properties.sources` become
/// `legal.documents.<any>.sources`, which is how a helm-unittest `set:` block and a template's
/// `.Values` access both spell it. A list element may carry its index, `[0]`, or not.
fn set_pattern(values_path: &str, position: &[Step]) -> Search {
    let mut parts = vec![regex::escape(values_path)];
    for step in position {
        match step {
            Step::Property(name) => parts.push(regex::escape(name)),
            Step::AdditionalProperties => parts.push(r#"[^.\s:"']+"#.to_owned()),
            Step::Items => {
                if let Some(last) = parts.last_mut() {
                    last.push_str(r"(?:\[\d+\])?");
                }
            }
        }
    }
    Search::bounded(&parts.join(r"\."), true, true)
}

/// A field read straight off a range variable (`$doc.sources`), or a dict key (`"sources"`).
///
/// Straight off the variable, because that is how a template holds one element of a map or a list,
/// and because a deeper path on some other variable — `$tls.ca.bundle.sources` in a template that
/// also reads `legal.documents` — is a different setting that happens to share the name. Measured
/// on `tankovault`: the looser `.sources` reported exactly that line, and a finding nobody can clear
/// would keep this red on every branch after the migration.
fn field_pattern(name: &str) -> AnyOf {
    let token = regex::escape(name);
    AnyOf(vec![
        Search::bounded(&format!(r"\$\w+\.{token}"), false, true),
        Search::plain(&format!("\"{token}\"")),
    ])
}

/// A mapping key (`sources:`) in a values file.
fn yaml_key_pattern(name: &str) -> Search {
    Search::plain(&format!(r"^\s*{}\s*:", regex::escape(name)))
}

/// The chart value itself, dotted: `legal.dir`, and not `legal.directory` or `x-legal.dir`.
fn dotted_value(values_path: &str) -> Search {
    Search::bounded(&regex::escape(values_path), true, true)
}

/// Every line of `file` that `pattern` matches.
fn lines_of(file: &File, pattern: &AnyOf) -> Vec<Reference> {
    file.text
        .lines()
        .enumerate()
        .filter(|(_, line)| pattern.found_in(line))
        .map(|(index, line)| Reference {
            path: file.path.clone(),
            line: index + 1,
            text: line.trim().to_owned(),
        })
        .collect()
}

/// Every line naming the chart value a removed key was bound to.
fn key_references(files: &Files, values_path: &str) -> Vec<Reference> {
    let dotted = AnyOf(vec![dotted_value(values_path)]);
    files
        .0
        .iter()
        .flat_map(|file| lines_of(file, &dotted))
        .collect()
}

/// Every line still using an element property removed at `position` under `values_path`.
///
/// Each kind of file spells it differently, so each gets the pattern for its own spelling, and a
/// file that never reads the value is searched only for the dotted form — which is what keeps a
/// projected volume's `sources:` out of a report about `legal.documents.*.sources`.
fn property_references(
    files: &Files,
    values_path: &str,
    position: &[Step],
    name: &str,
) -> Vec<Reference> {
    let reads_value = Search::bounded(
        &format!(r"Values\.{}", regex::escape(values_path)),
        false,
        true,
    );
    let top = values_path.split('.').next().unwrap_or(values_path);
    let top_block = Search::plain(&format!(r"(?m)^{}\s*:", regex::escape(top)));

    let mut found = Vec::new();
    for file in &files.0 {
        let mut pattern = vec![set_pattern(values_path, position)];
        // A template that reads the value holds its element in a range variable; a CI values file
        // spells the value nested, never dotted, under the value's top-level block.
        match file.origin {
            Origin::Template if reads_value.found_in(&file.text) => {
                pattern.extend(field_pattern(name).0);
            }
            Origin::CiValues if top_block.found_in(&file.text) => {
                pattern.push(yaml_key_pattern(name));
            }
            _ => {}
        }
        found.extend(lines_of(file, &AnyOf(pattern)));
    }
    found
}

// ------------------------------------------------------------------------------------------
// The report
// ------------------------------------------------------------------------------------------

/// The report a person reads before deciding what the templates should now say.
///
/// Findings with a remaining reference come first and are the failure. The rest are what the
/// contract took away that nothing in the chart still names — listed so a reader can confirm the
/// migration is done, and quiet once it is, so a branch that has followed its contract is not red
/// for the rest of its life.
///
/// Paths are shown relative to `root` when they are under it, which is the working directory a
/// person ran this from.
pub fn render(found: &[Migration], root: &Path) -> String {
    if found.is_empty() {
        return "==> no contract change removes anything a chart uses\n".to_owned();
    }
    let open: Vec<&Migration> = found.iter().filter(|m| !m.references.is_empty()).collect();
    let done: Vec<&Migration> = found.iter().filter(|m| m.references.is_empty()).collect();

    let mut lines: Vec<String> = Vec::new();
    if !open.is_empty() {
        lines.push(
            "==> hand migration required: the contract took these away, and no generator edits"
                .to_owned(),
        );
        lines.push(
            "    the templates, hand-written tests or CI values that still use them".to_owned(),
        );
        for migration in open {
            lines.push(format!("  {}", migration.headline()));
            for reference in &migration.references {
                lines.push(format!(
                    "      {}:{}: {}",
                    shown(&reference.path, root),
                    reference.line,
                    reference.text
                ));
            }
        }
    }
    if !done.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines
            .push("==> removed by the contract, and no longer referenced by the chart:".to_owned());
        for migration in done {
            let suffix = if migration.values_path.is_some() {
                ""
            } else {
                " (no chart value was bound to it)"
            };
            lines.push(format!("  {}{suffix}", migration.headline()));
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The same findings as JSON, for something that is not a person.
pub fn as_json(found: &[Migration], root: &Path) -> Json {
    serde_json::json!({
        "open": found.iter().any(|migration| !migration.references.is_empty()),
        "migrations": found.iter().map(|migration| serde_json::json!({
            "chart": migration.chart,
            "key": migration.key,
            "what": migration.what,
            "value": migration.values_path,
            "references": migration.references.iter().map(|reference| serde_json::json!({
                "path": shown(&reference.path, root),
                "line": reference.line,
                "text": reference.text,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// A path relative to `root` when it is under it, forward-slashed either way.
fn shown(path: &Path, root: &Path) -> String {
    let relative = match (path.canonicalize(), root.canonicalize()) {
        (Ok(path), Ok(root)) => path.strip_prefix(&root).map(Path::to_path_buf).ok(),
        _ => None,
    };
    relative
        .as_deref()
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{AnyOf, Step, field_pattern, is_generated_suite, removed_properties, set_pattern};

    fn property(name: &str) -> Step {
        Step::Property(name.to_owned())
    }

    fn old_element() -> serde_json::Value {
        json!({
            "type": "object",
            "additionalProperties": {
                "type": "object",
                "additionalProperties": false,
                "properties": {"sources": {"type": "object"}, "url": {"type": "string"}},
            },
        })
    }

    fn new_element() -> serde_json::Value {
        json!({
            "type": "object",
            "additionalProperties": {
                "type": "object",
                "additionalProperties": false,
                "properties": {"body": {"type": "object"}, "url": {"type": "string"}},
            },
        })
    }

    #[test]
    fn a_property_the_element_lost_is_reported() {
        assert_eq!(
            removed_properties(Some(&old_element()), Some(&new_element())),
            vec![vec![Step::AdditionalProperties, property("sources")]]
        );
    }

    #[test]
    fn an_addition_is_not_a_removal() {
        assert_eq!(
            removed_properties(Some(&new_element()), Some(&new_element())),
            Vec::<Vec<Step>>::new()
        );
    }

    #[test]
    fn a_vanished_parent_is_reported_once() {
        let old = json!({"properties": {"tls": {"properties": {"ca": {}, "cert": {}}}}});
        assert_eq!(
            removed_properties(Some(&old), Some(&json!({"properties": {}}))),
            vec![vec![property("tls")]]
        );
    }

    #[test]
    fn items_are_walked() {
        let old = json!({"items": {"properties": {"a": {}, "b": {}}}});
        let new = json!({"items": {"properties": {"a": {}}}});
        assert_eq!(
            removed_properties(Some(&old), Some(&new)),
            vec![vec![Step::Items, property("b")]]
        );
    }

    #[test]
    fn a_property_named_like_a_keyword_is_a_name() {
        // A flat list of keyword strings could not tell `properties.items` from `items`; a typed
        // step can, so the dotted spelling names the property rather than an array element.
        let old = json!({"properties": {"items": {}}});
        let position = removed_properties(Some(&old), Some(&json!({})));
        assert_eq!(position, vec![vec![property("items")]]);
        let pattern = set_pattern("queue", &position[0]);
        assert!(pattern.found_in("queue.items: 3"));
        assert!(!pattern.found_in("queue[0]: 3"));
    }

    #[test]
    fn a_schema_that_is_not_an_object_on_either_side_has_nothing_removed() {
        assert_eq!(
            removed_properties(Some(&old_element()), None),
            Vec::<Vec<Step>>::new()
        );
        assert_eq!(
            removed_properties(Some(&json!(true)), Some(&old_element())),
            Vec::<Vec<Step>>::new()
        );
    }

    #[test]
    fn the_set_pattern_spans_the_map_key() {
        let pattern = set_pattern(
            "legal.documents",
            &[Step::AdditionalProperties, property("sources")],
        );
        assert!(pattern.found_in("legal.documents.privacy.sources.en: /x"));
        assert!(!pattern.found_in("legal.documents.privacy.body.en: x"));
        assert!(!pattern.found_in("legal.documents.privacy.sourcesExtra: x"));
    }

    #[test]
    fn the_set_pattern_admits_an_index_on_a_list_element() {
        let pattern = set_pattern("routes", &[Step::Items, property("weight")]);
        assert!(pattern.found_in("routes[0].weight: 1"));
        assert!(pattern.found_in("routes.weight"));
        assert!(!pattern.found_in("routes[x].weight"));
    }

    #[test]
    fn the_set_pattern_is_a_whole_name_on_both_sides() {
        let pattern = set_pattern("legal.dir", &[]);
        assert!(pattern.found_in("{{ .Values.legal.dir }}"));
        assert!(!pattern.found_in("x-legal.dir"));
        assert!(!pattern.found_in("legal.directory"));
        // A rejected candidate does not hide an accepted one later on the same line.
        assert!(pattern.found_in("my-legal.dir and legal.dir"));
    }

    #[test]
    fn the_field_pattern_needs_a_range_variable() {
        let pattern: AnyOf = field_pattern("sources");
        assert!(pattern.found_in("{{ $doc.sources }}"));
        assert!(pattern.found_in(r#"{{ set $entry "sources" . }}"#));
        assert!(!pattern.found_in("{{ $tls.ca.bundle.sources }}"));
        assert!(!pattern.found_in("{{ $sources := dict }}"));
    }

    #[test]
    fn only_the_contract_round_trip_suites_count_as_generated() {
        for generated in [
            "contract_roundtrip_api_test.yaml",
            "contract_roundtrip__test.yaml",
        ] {
            assert!(
                is_generated_suite(std::path::Path::new(generated)),
                "{generated}"
            );
        }
        for written in [
            "contract_roundtrip_test.yaml",
            "legal_test.yaml",
            "contract_roundtrip_api_test.yml",
        ] {
            assert!(
                !is_generated_suite(std::path::Path::new(written)),
                "{written}"
            );
        }
    }
}
