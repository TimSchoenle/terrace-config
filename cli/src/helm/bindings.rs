//! Every `@config` marker, against the contract it names.
//!
//! Coverage is chart-level: does this chart carry a declaration at all. So the failure a chart
//! repository actually keeps hitting is invisible to it — an image release adds a setting, an
//! automated bump repins the digest and omits everything else, and nothing notices that the new key
//! is reached by no chart value. The document gate cannot see it either: **a key nothing renders is
//! not a key rendered wrongly.**
//!
//! This is the key-level half. Five rules, and every one of them is a way for a marker to be worse
//! than no marker at all:
//!
//! 1. **A marker's target exists.** A typo binds a value to a key that is not there while looking
//!    exactly like coverage.
//! 2. **The class matches the key.** `structured` must name a key the contract calls `structured`,
//!    and `projection` must not: a scalar written where a map belongs is a defect the marker is in
//!    a position to state, and therefore has to be held to.
//! 3. **A `when` clause names a values path that exists.** A gate value that was renamed leaves the
//!    marker asserting a condition nothing evaluates.
//! 4. **One key is bound by one value — except under `composed`.** That exception is the entire
//!    reason `composed` is a class: a value built by `printf "%s:%v" host port` has two inputs, and
//!    both must say so. The converse rule — that a lone `composed` is really a projection — was
//!    *removed*, because a real chart falsifies it: one value and a literal the template supplies
//!    is still a composition. `composed` says "an input", and the arity is counted here.
//! 5. **Every key of an enrolled chart is bound, or written off with a reason.** This is the rule
//!    the whole module exists for; the other four keep it from being satisfied by fiction.
//!
//! # Enrolment is declared, not inferred
//!
//! `bindings: true` in the declaration. Inferring it from "the chart carries at least one marker"
//! reads well until a chart *stops* carrying them — a botched edit, a rewritten values file, a
//! marker spelt in a way the reader does not recognise — and the chart drops out of the report
//! without a word. With the switch, both disagreements are failures: markers without the switch,
//! and the switch without markers.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value as Json;

use crate::error::Error;
use crate::report::Report;
use crate::union::suggest;

use super::declaration::{Bound, Declaration, chart_dirs, load_declaration, read_yaml};
use super::markers::{Class, MARKER, Marker, read as read_markers};

/// What a run of [`check`] found.
#[derive(Debug)]
pub struct Bindings {
    /// Everything the rules had to say.
    pub report: Report,
    /// One row per enrolled chart: its name, how many contract keys it binds, and how many declared
    /// external variables.
    pub enrolled: Vec<(String, usize, usize)>,
}

/// One marker, and one document it resolved its target in.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// The marker.
    pub marker: Marker,
    /// The document its target was found in.
    pub document: String,
}

/// Hold every enrolled chart's markers against the contracts they name.
///
/// # Errors
/// [`Error::Invalid`] when a declaration, a values file or a vendored contract cannot be read at
/// all — as distinct from a marker that is wrong, which is a finding.
pub fn check(charts: &Path) -> Result<Bindings, Error> {
    let mut report = Report::new();
    let mut enrolled = Vec::new();

    for chart_dir in chart_dirs(charts)? {
        if let Some(counted) = check_chart(&chart_dir, &mut report)? {
            let name = chart_dir
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            enrolled.push((name, counted.0, counted.1));
        }
    }

    Ok(Bindings { report, enrolled })
}

/// Check one chart; [`None`] when its declaration does not enrol it.
fn check_chart(chart_dir: &Path, report: &mut Report) -> Result<Option<(usize, usize)>, Error> {
    let values_path = chart_dir.join("values.yaml");
    if !values_path.is_file() {
        return Ok(None);
    }
    let chart = chart_dir
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let at = format!("{chart}: values.yaml");

    let text =
        std::fs::read_to_string(&values_path).map_err(|e| Error::io(values_path.display(), e))?;
    let (markers, _) = read_markers(&text, &chart)?;
    let declaration = load_declaration(chart_dir)?;

    let enrolled = declaration
        .as_ref()
        .is_some_and(|declaration| declaration.bindings);
    if !enrolled {
        if markers.is_empty() {
            return Ok(None);
        }
        report.fail(
            &at,
            format!(
                "carries {} `{MARKER}` marker(s), and its {}. Enrolment is declared rather than \
                 inferred, so that a chart which loses its markers goes red instead of quietly \
                 leaving the report",
                markers.len(),
                if declaration.is_some() {
                    "config-contract.yaml does not declare `bindings: true`"
                } else {
                    "chart has no config-contract.yaml to declare `bindings: true` in"
                }
            ),
        );
        return Ok(Some((0, 0)));
    }

    let declaration = declaration.expect("enrolment was read from a declaration");
    if declaration.documents.is_empty() {
        report.fail(
            &at,
            format!(
                "declares `bindings: true` but no configuration contract, so there is nothing to \
                 hold a `{MARKER}` marker against; declare a document, or drop the switch"
            ),
        );
        return Ok(Some((0, 0)));
    }
    if markers.is_empty() {
        report.fail(
            &at,
            format!(
                "declares `bindings: true` and carries no `{MARKER}` marker. Either the markers \
                 were lost — which is the drift this switch exists to catch — or the chart was \
                 never bound and the switch should not be set"
            ),
        );
        return Ok(Some((0, 0)));
    }

    let bound = Bound::of(chart_dir, declaration)?;
    let values = read_yaml(&text, &values_path)?;

    let resolved = resolve(&bound, &markers, &values, report);
    check_uniqueness(&resolved, report);
    check_write_offs(&bound, report);
    check_coverage(&bound, &resolved, report);

    let keys = resolved
        .iter()
        .filter(|held| held.marker.class.targets_a_key())
        .count();
    Ok(Some((keys, resolved.len() - keys)))
}

// ------------------------------------------------------------------------------------------
// Rules 1 to 3 — one marker at a time
// ------------------------------------------------------------------------------------------

/// Every marker that named something real, once per document it binds the key in.
fn resolve(bound: &Bound, markers: &[Marker], values: &Json, report: &mut Report) -> Vec<Resolved> {
    let mut resolved = Vec::new();
    for marker in markers {
        let documents = resolve_documents(bound, marker, report);
        if documents.is_empty() || !check_shape(bound, marker, &documents, report) {
            continue;
        }
        check_condition(marker, values, report);
        for document in documents {
            resolved.push(Resolved {
                marker: marker.clone(),
                document,
            });
        }
    }
    resolved
}

/// Which documents the marker binds its target in; empty when it named nothing real.
///
/// An unscoped marker binds **every** document whose contract declares the target, which is what a
/// chart does: one template line writes a key into every service that reads it. An earlier design
/// refused that and asked for a qualifier, which made the case inexpressible — a value carries one
/// marker, and a qualified marker covers one document and leaves the rest owing.
///
/// A scope narrows it, for the case the default over-claims. Naming a document that does not declare
/// the target is refused rather than ignored, because that is how a scope goes stale.
fn resolve_documents(bound: &Bound, marker: &Marker, report: &mut Report) -> Vec<String> {
    let declared: Vec<String> = bound.documents.keys().cloned().collect();

    let candidates: Vec<String> = match &marker.documents {
        Some(scope) => {
            let unknown: Vec<String> = scope
                .iter()
                .filter(|name| !bound.documents.contains_key(*name))
                .map(|name| crate::gate::quoted(name))
                .collect();
            if !unknown.is_empty() {
                report.fail(
                    marker.at(),
                    format!(
                        "is scoped to {}, which this chart does not declare (declared: {})",
                        unknown.join(", "),
                        declared.join(", ")
                    ),
                );
                return Vec::new();
            }
            scope.clone()
        }
        None => declared,
    };

    let matched: Vec<String> = candidates
        .iter()
        .filter(|name| {
            bound
                .namespace(name, marker.class.targets_a_key())
                .is_some_and(|namespace| namespace.contains(&marker.target))
        })
        .cloned()
        .collect();

    if !matched.is_empty() && marker.documents.is_some() && matched.len() != candidates.len() {
        let missing: Vec<&str> = candidates
            .iter()
            .filter(|name| !matched.contains(name))
            .map(String::as_str)
            .collect();
        report.fail(
            marker.at(),
            format!(
                "is scoped to {}, whose contract does not declare {}; a scope that names a \
                 document the key is absent from is a scope nobody has re-read since the contract \
                 changed",
                missing.join(", "),
                crate::gate::quoted(&marker.target)
            ),
        );
        return Vec::new();
    }
    if !matched.is_empty() {
        return matched;
    }

    let namespace = if marker.class.targets_a_key() {
        "contract key"
    } else {
        "external.env variable"
    };
    let mut known: Vec<String> = Vec::new();
    for name in &candidates {
        if let Some(held) = bound.namespace(name, marker.class.targets_a_key()) {
            known.extend(held.names().map(str::to_owned));
        }
    }
    let scope = match &marker.documents {
        Some(scope) => format!("the document(s) {} do not declare", scope.join(", ")),
        None => "no contract this chart declares carries".to_owned(),
    };
    report.fail(
        marker.at(),
        format!(
            "binds the value {} to the {namespace} {}, which {scope}{}",
            crate::gate::quoted(&marker.values_path),
            crate::gate::quoted(&marker.target),
            suggest(&marker.target, known.iter().map(String::as_str))
        ),
    );
    Vec::new()
}

/// Rule 2. A `structured` key holds a map the operator names; a scalar key does not.
///
/// Checked against every document the marker binds, and those documents are first required to agree
/// with each other. Two contracts declaring one path in two different shapes are two different
/// settings that happen to share a name, and binding both from one value would be the marker
/// asserting something no reader could have meant.
fn check_shape(bound: &Bound, marker: &Marker, documents: &[String], report: &mut Report) -> bool {
    let forms: BTreeMap<&str, Option<&str>> = documents
        .iter()
        .map(|document| {
            let form = bound
                .namespace(document, marker.class.targets_a_key())
                .and_then(|namespace| namespace.get(&marker.target))
                .and_then(|entry| entry.text("text_form"));
            (document.as_str(), form)
        })
        .collect();

    let distinct: BTreeSet<Option<&str>> = forms.values().copied().collect();
    if distinct.len() > 1 {
        let spelt: Vec<String> = forms
            .iter()
            .map(|(document, form)| format!("{document}: {}", form.unwrap_or("None")))
            .collect();
        report.fail(
            marker.at(),
            format!(
                "binds {} in documents that do not agree what it is ({}); one path in two shapes \
                 is two settings, so scope the marker to the one this value feeds",
                crate::gate::quoted(&marker.target),
                spelt.join(", ")
            ),
        );
        return false;
    }

    let form = distinct.into_iter().next().flatten();
    let structured = form == Some("structured");

    if marker.class == Class::Structured && !structured {
        report.fail(
            marker.at(),
            format!(
                "is `{}`, but the contract calls {} a scalar ({}); a scalar key takes `{}`",
                Class::Structured.label(),
                crate::gate::quoted(&marker.target),
                form.unwrap_or("None"),
                Class::Projection.label()
            ),
        );
        return false;
    }
    if marker.class == Class::Projection && structured {
        report.fail(
            marker.at(),
            format!(
                "is `{}`, but the contract calls {} structured — the keys underneath it are the \
                 operator's own names, which is what `{}` says",
                Class::Projection.label(),
                crate::gate::quoted(&marker.target),
                Class::Structured.label()
            ),
        );
        return false;
    }
    true
}

/// Rule 3. The gate value has to still be spelt the way the marker says it is.
fn check_condition(marker: &Marker, values: &Json, report: &mut Report) {
    let Some(condition) = &marker.condition else {
        return;
    };
    if has_path(values, condition) {
        return;
    }
    report.fail(
        marker.at(),
        format!(
            "is written only `when {condition}`, and this chart has no value at that path; a \
             condition nothing evaluates is a subtree nobody can predict"
        ),
    );
}

/// Whether a dotted values path exists, including one whose value is null or false.
///
/// Presence rather than truth: a gate value set to `false` is still a gate the chart has, and a
/// reader that folded the two would report a renamed value and a switched-off one identically.
pub fn has_path(values: &Json, path: &str) -> bool {
    let mut current = values;
    for part in path.split('.') {
        let Some(next) = current.as_object().and_then(|fields| fields.get(part)) else {
            return false;
        };
        current = next;
    }
    true
}

// ------------------------------------------------------------------------------------------
// Rule 4 — markers against each other
// ------------------------------------------------------------------------------------------

/// One key, one value — unless every value binding it says `composed`.
///
/// Only this direction is checked here. The other one — a value binding two keys — belongs to the
/// marker reader, because two markers in one block never reach this far.
fn check_uniqueness(resolved: &[Resolved], report: &mut Report) {
    let mut by_target: BTreeMap<(&str, &str), Vec<&Marker>> = BTreeMap::new();
    for held in resolved {
        // Keyed by the document the target *resolved* to rather than by how it was written, so a
        // scoped and an unscoped marker naming one key collide as they should.
        by_target
            .entry((held.document.as_str(), held.marker.target.as_str()))
            .or_default()
            .push(&held.marker);
    }

    for ((document, target), markers) in by_target {
        let composed = markers
            .iter()
            .filter(|marker| marker.class == Class::Composed)
            .count();
        if markers.len() > 1 && composed != markers.len() {
            report.fail(
                markers[0].at(),
                format!(
                    "{document}: {} is bound by {} values ({}); only `{}` admits several, and \
                     then every one of them must say so",
                    crate::gate::quoted(target),
                    markers.len(),
                    markers
                        .iter()
                        .map(|marker| marker.values_path.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    Class::Composed.label()
                ),
            );
        }
    }
}

// ------------------------------------------------------------------------------------------
// Rule 5 — the coverage rule
// ------------------------------------------------------------------------------------------

/// Every `unbound` key names something some contract in its scope declares.
///
/// A chart-level question rather than a per-document one, and the distinction is the whole reason
/// this is its own pass. An unscoped entry writes a key off *wherever it is declared*, so a document
/// that does not declare it is not an error — it is the ordinary case for a chart whose services
/// share a configuration crate and read different halves of it. An entry no document at all
/// recognises is a different thing: a key renamed or removed upstream, still written off here,
/// quietly covering nothing.
fn check_write_offs(bound: &Bound, report: &mut Report) {
    let mut declared: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (name, union) in &bound.unions {
        for key in union.keys.names() {
            declared.entry(key).or_default().insert(name.as_str());
        }
    }

    let at = format!("{}: {}", bound.chart, super::DECLARATION);
    for entry in &bound.declaration.unbound {
        let scope: BTreeSet<&str> = match &entry.documents {
            Some(named) => named.iter().map(String::as_str).collect(),
            None => bound.documents.keys().map(String::as_str).collect(),
        };
        for key in &entry.keys {
            let holders = declared
                .get(key.as_str())
                .is_some_and(|holders| holders.iter().any(|holder| scope.contains(holder)));
            if holders {
                continue;
            }
            let named = crate::gate::quoted(key);
            let hint = suggest(key, declared.keys().copied());
            if entry.documents.is_some() {
                report.fail(
                    &at,
                    format!(
                        "`unbound` writes off {named} in {}, whose contracts do not declare it{hint}",
                        scope.iter().copied().collect::<Vec<_>>().join(", ")
                    ),
                );
            } else {
                report.fail(
                    &at,
                    format!(
                        "`unbound` writes off {named}, which no contract this chart declares \
                         carries{hint}"
                    ),
                );
            }
        }
    }
}

/// Every contract key of an enrolled chart is bound, or written off with a reason.
///
/// Over the keys only. A declared external variable is a namespace the loader does not own, so a
/// chart that offers no value for one has declined to expose somebody else's variable — which is
/// not the omission this rule is looking for.
fn check_coverage(bound: &Bound, resolved: &[Resolved], report: &mut Report) {
    let missing = missing_keys(bound, resolved);

    for (name, union) in &bound.unions {
        let at = format!("{}: {name}", bound.chart);
        let bound_here = bound_in(resolved, name);
        let written_off = written_off_in(&bound.declaration, name);

        for key in union.keys.names() {
            if written_off.contains(key) && bound_here.contains(key) {
                report.fail(
                    format!("{at}: {}", super::DECLARATION),
                    format!(
                        "`unbound` names {} while a marker binds it; one of the two is out of \
                         date, and the reason recorded here is the one that will be believed",
                        crate::gate::quoted(key)
                    ),
                );
            }
        }

        for key in missing.get(name).into_iter().flatten() {
            let secret = union
                .keys
                .get(key)
                .and_then(|entry| entry.fields.get("secret"))
                .and_then(Json::as_bool)
                .unwrap_or(false);
            report.fail(
                &at,
                format!(
                    "the contract declares {} and no chart value binds it. Add a `# # {MARKER} \
                     ...` marker to the `@schema` block of the value that feeds it, or an \
                     `unbound` entry with a reason in {}{}",
                    crate::gate::quoted(key),
                    super::DECLARATION,
                    if secret {
                        " (it is a credential; the secrets inventory is what checks how those are \
                         delivered)"
                    } else {
                        ""
                    }
                ),
            );
        }
    }
}

/// The targets bound in one document, by a marker whose class names a key.
fn bound_in<'a>(resolved: &'a [Resolved], document: &str) -> BTreeSet<&'a str> {
    resolved
        .iter()
        .filter(|held| held.document == document && held.marker.class.targets_a_key())
        .map(|held| held.marker.target.as_str())
        .collect()
}

/// The keys of each document that no value binds and no `unbound` entry writes off.
///
/// Lifted out of the rule that reports it because a second caller acts on the same set: the writer
/// that adds the values which would satisfy it. Two readers of "which keys are owed" would agree
/// today and disagree the first time a scope or a write-off changed, and the one that drifts is the
/// writer — it would either scaffold a value for a key somebody deliberately wrote off, or leave the
/// gate red after a run that reported success.
///
/// Keyed by document and never sparse: a document whose keys are all bound maps to an empty list, so
/// a caller can walk a chart's documents without asking whether each is present.
pub fn missing_keys(bound: &Bound, resolved: &[Resolved]) -> BTreeMap<String, Vec<String>> {
    let mut missing = BTreeMap::new();
    for (name, union) in &bound.unions {
        let bound_here = bound_in(resolved, name);
        let written_off = written_off_in(&bound.declaration, name);
        let owed: Vec<String> = union
            .keys
            .names()
            .filter(|key| !bound_here.contains(key) && !written_off.contains(*key))
            .map(str::to_owned)
            .collect();
        let mut owed = owed;
        owed.sort();
        missing.insert(name.clone(), owed);
    }
    missing
}

/// The keys `unbound` writes off in one document.
///
/// An entry with no scope writes its keys off wherever they are declared, which is what "no chart
/// value surfaces this" means for a chart whose services share a configuration crate. A scoped entry
/// writes them off only in the documents it names.
pub fn written_off_in(declaration: &Declaration, document: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for entry in &declaration.unbound {
        let in_scope = entry
            .documents
            .as_ref()
            .is_none_or(|named| named.iter().any(|name| name == document));
        if in_scope {
            keys.extend(entry.keys.iter().cloned());
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::has_path;

    #[test]
    fn a_values_path_that_is_null_or_false_still_exists() {
        // Presence rather than truth: a gate set to `false` is still a gate the chart has, and a
        // reader that folded the two would report a renamed value and a switched-off one
        // identically.
        let values = json!({"bootstrap": {"seedAdmin": {"enabled": false, "email": null}}});
        assert!(has_path(&values, "bootstrap.seedAdmin.enabled"));
        assert!(has_path(&values, "bootstrap.seedAdmin.email"));
        assert!(!has_path(&values, "bootstrap.seedAdmin.missing"));
        assert!(!has_path(&values, "nothing"));
    }
}
