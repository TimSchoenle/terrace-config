//! Comparing the keys and external names one by one, field by field.

use serde_json::{Map, Value as Json};

use crate::classify::matches_ignore;
use crate::document::{entry_name_pattern, required_entries};
use crate::text::{quoted, short};

use super::tightening::{diff_tightenings, without_refinements};
use super::{Area, Change, Kind, Severity, indexed, nested, shape_of, text_of, truthy};

/// Per-field severity for a key that exists on both sides.
///
/// Three groups, and the reasoning for each is the reasoning the gates apply at deployment time:
///
/// - **major**: the chart's own spelling of the setting stopped being the one the image reads. A
///   spelling change should be unreachable without the path or the dialect moving, and if it
///   happens anyway it is exactly as severe as a dialect change.
/// - **minor**: the value the chart writes is still spelled right but may no longer be accepted, or
///   a default moved out from under a chart that relied on it.
/// - **patch**: prose. Real, and not a reason to bump anything.
///
/// `required` and `text_form` are absent because neither has a fixed severity; both are graded by
/// direction.
pub(super) const KEY_FIELDS: [(&str, Severity); 15] = [
    ("aliases", Severity::Minor),
    ("constraint", Severity::Minor),
    ("docs", Severity::Patch),
    ("env", Severity::Major),
    ("env_aliases", Severity::Minor),
    ("env_file", Severity::Major),
    ("env_file_aliases", Severity::Minor),
    ("note", Severity::Patch),
    ("reserved", Severity::Minor),
    ("secret", Severity::Minor),
    ("secrets_file", Severity::Major),
    ("secrets_file_aliases", Severity::Minor),
    ("text_constraint", Severity::Minor),
    ("ty", Severity::Minor),
    ("values", Severity::Minor),
];

/// Per-field severity for an external variable.
///
/// A smaller entry, and its spellings are its own name rather than something derived, so there is
/// no equivalent of the major group here.
pub(super) const EXTERNAL_FIELDS: [(&str, Severity); 7] = [
    ("constraint", Severity::Minor),
    ("docs", Severity::Patch),
    ("owner", Severity::Patch),
    ("secret", Severity::Minor),
    ("text_constraint", Severity::Minor),
    ("ty", Severity::Minor),
    ("values", Severity::Minor),
];

/// Compared as a pair rather than as two fields: one is the text the loader would use and the other
/// is that text parsed, so they move together and reporting both is one fact printed twice.
pub(super) const DEFAULT_FIELDS: [&str; 2] = ["default", "default_value"];

/// The key declarations: the half of the document a chart's rendered settings answer to.
pub(super) fn diff_keys(old: &Map<String, Json>, new: &Map<String, Json>) -> Vec<Change> {
    let old_keys = indexed(old.get("schema"), "keys", "path");
    let new_keys = indexed(new.get("schema"), "keys", "path");
    let mut changes = Vec::new();

    for (path, entry) in &new_keys {
        if old_keys.contains_key(path) {
            continue;
        }
        // A new *required* key is the same deployment failure as one that became required: the
        // chart does not write it, and the image refuses to start without it.
        let required = truthy(entry.get("required"));
        changes.push(Change {
            severity: if required {
                Severity::Major
            } else {
                Severity::Minor
            },
            area: Area::Key,
            kind: Kind::Added,
            subject: path.clone(),
            field: None,
            old: None,
            new: Some(entry.clone()),
            message: format!(
                "{path} added ({}); the chart writes nothing for it{}",
                shape_of(entry),
                if required {
                    ", and it is required, so the image will not start until the chart does"
                } else {
                    ""
                }
            ),
        });
    }

    for (path, entry) in &old_keys {
        if new_keys.contains_key(path) {
            continue;
        }
        // No downgrade is possible. The old spelling still begins with the prefix, so the
        // classification stops at step 4, above both external lists, and a `reject` policy turns it
        // into a container that does not start.
        changes.push(Change {
            severity: Severity::Major,
            area: Area::Key,
            kind: Kind::Removed,
            subject: path.clone(),
            field: None,
            old: Some(entry.clone()),
            new: None,
            message: format!(
                "{path} is gone; a chart still emitting {} or mounting {} is refused at boot, not \
                 ignored",
                text_of(entry.get("env")),
                text_of(entry.get("secrets_file"))
            ),
        });
    }

    for (path, before) in &old_keys {
        if let Some(after) = new_keys.get(path) {
            changes.extend(diff_entry(Area::Key, path, before, after, &KEY_FIELDS));
        }
    }
    changes
}

/// The variables the image reads that are nobody's configuration.
///
/// A removal is graded by what the classification does with the name afterwards. Having fallen past
/// the prefix step, it reaches the ignore patterns, so a surviving pattern that matches absorbs it
/// and the removal costs nothing; with no pattern matching, it reaches the unknown-variable policy
/// and a chart still setting it stops booting.
pub(super) fn diff_external(old: &Map<String, Json>, new: &Map<String, Json>) -> Vec<Change> {
    let old_env = indexed(old.get("external"), "env", "name");
    let new_env = indexed(new.get("external"), "env", "name");
    let external = nested(new, "external");
    let policy = external.get("unknown").and_then(Json::as_str).unwrap_or("");
    let patterns: Vec<&str> = external
        .get("ignore")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
        .collect();
    let mut changes = Vec::new();

    for (name, entry) in &new_env {
        if old_env.contains_key(name) {
            continue;
        }
        let required = truthy(entry.get("required"));
        changes.push(Change {
            severity: if required {
                Severity::Major
            } else {
                Severity::Minor
            },
            area: Area::External,
            kind: Kind::Added,
            subject: name.clone(),
            field: None,
            old: None,
            new: Some(entry.clone()),
            message: format!(
                "external variable {name} added, owned by {}{}",
                short(entry.get("owner")),
                if required { " and required" } else { "" }
            ),
        });
    }

    for (name, entry) in &old_env {
        if new_env.contains_key(name) {
            continue;
        }
        let absorbed = patterns.iter().any(|pattern| matches_ignore(pattern, name));
        let rejected = policy == "reject" && !absorbed;
        changes.push(Change {
            severity: if rejected {
                Severity::Major
            } else {
                Severity::Minor
            },
            area: Area::External,
            kind: Kind::Removed,
            subject: name.clone(),
            field: None,
            old: Some(entry.clone()),
            new: None,
            message: format!(
                "external variable {name} is no longer declared; {}",
                if rejected {
                    format!(
                        "a chart still setting it is refused at boot (external.unknown is {})",
                        quoted(policy)
                    )
                } else if absorbed {
                    "a surviving ignore pattern still absorbs it".to_owned()
                } else {
                    format!("the image's unknown-variable policy is {}", quoted(policy))
                }
            ),
        });
    }

    for (name, before) in &old_env {
        if let Some(after) = new_env.get(name) {
            changes.extend(diff_entry(
                Area::External,
                name,
                before,
                after,
                &EXTERNAL_FIELDS,
            ));
        }
    }
    changes
}

/// One declaration that exists on both sides, field by field.
pub(super) fn diff_entry(
    area: Area,
    subject: &str,
    old: &Json,
    new: &Json,
    fields: &[(&str, Severity)],
) -> Vec<Change> {
    let mut changes = Vec::new();

    for (name, severity) in fields {
        // A map's required entries are graded by direction below, so a constraint differing in
        // nothing else is not a second, flat-graded finding about the same edit.
        let differs = if *name == "constraint" {
            without_refinements(old.get(name)) != without_refinements(new.get(name))
        } else {
            old.get(name) != new.get(name)
        };
        if differs {
            changes.push(Change {
                severity: *severity,
                area,
                kind: Kind::Changed,
                subject: subject.to_owned(),
                field: Some((*name).to_owned()),
                old: old.get(name).cloned(),
                new: new.get(name).cloned(),
                message: format!(
                    "{subject}: {name} {} -> {}",
                    short(old.get(name)),
                    short(new.get(name))
                ),
            });
        }
    }

    changes.extend(diff_required(area, subject, old, new));
    changes.extend(diff_required_entries(area, subject, old, new));
    changes.extend(diff_entry_names(area, subject, old, new));
    changes.extend(diff_tightenings(area, subject, old, new));
    changes.extend(diff_text_form(area, subject, old, new));
    changes.extend(diff_default(area, subject, old, new));

    // A field the producer added that this has no opinion about. Reported at patch so it is visible
    // without claiming a severity nobody has reasoned about. A diff that silently ignores what it
    // does not recognise is the failure this whole toolchain exists to remove.
    let mut known: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
    known.extend(DEFAULT_FIELDS);
    // `reload` is graded by `diff_reload`, by the class a consumer derives rather than by the
    // bytes, because the same class can be spelled two ways.
    known.extend([
        "path",
        "name",
        "required",
        "text_form",
        "unreachable",
        "reload",
    ]);
    let mut unknown: Vec<&String> = old
        .as_object()
        .into_iter()
        .flatten()
        .chain(new.as_object().into_iter().flatten())
        .map(|(name, _)| name)
        .filter(|name| !known.contains(&name.as_str()))
        .collect();
    unknown.sort();
    unknown.dedup();
    for name in unknown {
        if old.get(name) != new.get(name) {
            changes.push(Change {
                severity: Severity::Patch,
                area,
                kind: Kind::Changed,
                subject: subject.to_owned(),
                field: Some(name.clone()),
                old: old.get(name).cloned(),
                new: new.get(name).cloned(),
                message: format!(
                    "{subject}: {name} changed, and this diff models no severity for that field: \
                     {} -> {}",
                    short(old.get(name)),
                    short(new.get(name))
                ),
            });
        }
    }

    changes
}

/// `required` is graded by direction: gaining it breaks a chart, losing it cannot.
pub(super) fn diff_required(area: Area, subject: &str, old: &Json, new: &Json) -> Vec<Change> {
    let before = truthy(old.get("required"));
    let after = truthy(new.get("required"));
    if before == after {
        return Vec::new();
    }
    vec![Change {
        severity: if after {
            Severity::Major
        } else {
            Severity::Minor
        },
        area,
        kind: Kind::Changed,
        subject: subject.to_owned(),
        field: Some("required".to_owned()),
        old: Some(Json::Bool(before)),
        new: Some(Json::Bool(after)),
        message: if after {
            format!("{subject} is now required; a chart that does not write it will not start")
        } else {
            format!("{subject} is no longer required")
        },
    }]
}

/// A map's required entries are graded by direction, as `required` is.
///
/// An entry the map must now contain is a tightening: a chart rendering the map without it renders
/// a document the image refuses, so gaining one is major for the same reason gaining `required` is.
/// Losing one is a relaxation: a value that was refused may now be accepted, and nothing that
/// worked stops working.
pub(super) fn diff_required_entries(
    area: Area,
    subject: &str,
    old: &Json,
    new: &Json,
) -> Vec<Change> {
    let before = required_entries(old.get("constraint"));
    let after = required_entries(new.get("constraint"));
    let gained: Vec<&str> = after
        .iter()
        .filter(|entry| !before.contains(entry))
        .copied()
        .collect();
    let lost: Vec<&str> = before
        .iter()
        .filter(|entry| !after.contains(entry))
        .copied()
        .collect();

    let entries = |names: &[&str]| -> Json {
        Json::Array(
            names
                .iter()
                .map(|name| Json::String((*name).to_owned()))
                .collect(),
        )
    };
    let mut changes = Vec::new();
    if !gained.is_empty() {
        changes.push(Change {
            severity: Severity::Major,
            area,
            kind: Kind::Changed,
            subject: subject.to_owned(),
            field: Some("constraint.required".to_owned()),
            old: Some(entries(&before)),
            new: Some(entries(&after)),
            message: format!(
                "{subject} must now contain {}; a chart rendering the map without them will not start",
                gained.join(", ")
            ),
        });
    }
    if !lost.is_empty() {
        changes.push(Change {
            severity: Severity::Minor,
            area,
            kind: Kind::Changed,
            subject: subject.to_owned(),
            field: Some("constraint.required".to_owned()),
            old: Some(entries(&before)),
            new: Some(entries(&after)),
            message: format!("{subject} no longer has to contain {}", lost.join(", ")),
        });
    }
    changes
}

/// A map's entry-name pattern is graded by direction too, as far as a pattern has one.
///
/// Gaining a pattern is a tightening, and so is replacing one: whether a new pattern admits every
/// name the old one did is not something two regular expressions can be compared for here, and a
/// chart naming an entry the new one refuses renders a map the image will not start with. Losing
/// one is a relaxation.
pub(super) fn diff_entry_names(area: Area, subject: &str, old: &Json, new: &Json) -> Vec<Change> {
    let before = entry_name_pattern(old.get("constraint"));
    let after = entry_name_pattern(new.get("constraint"));
    if before == after {
        return Vec::new();
    }
    let pattern = |held: Option<&str>| held.map(|pattern| Json::String(pattern.to_owned()));
    let (severity, message) = match (before, after) {
        (_, Some(after)) => (
            Severity::Major,
            format!(
                "{subject} now holds its entry names to {after}; a chart naming an entry it \
                 refuses will not start"
            ),
        ),
        (Some(before), None) => (
            Severity::Minor,
            format!("{subject} no longer holds its entry names to {before}"),
        ),
        (None, None) => return Vec::new(),
    };
    vec![Change {
        severity,
        area,
        kind: Kind::Changed,
        subject: subject.to_owned(),
        field: Some("constraint.propertyNames".to_owned()),
        old: pattern(before),
        new: pattern(after),
        message,
    }]
}

/// `text_form` decides the read, and with it whether a file can supply the setting at all.
///
/// The escalation is the point. A file delivers its contents as a string with no parse, so only a
/// `text` key can be supplied by one. A key that moves from `text` to `integer` is still spelled the
/// same and still renders the same, and the secret the chart mounts for it stops loading. Graded
/// flat that would be minor, which is wrong for exactly this case.
pub(super) fn diff_text_form(area: Area, subject: &str, old: &Json, new: &Json) -> Vec<Change> {
    let before = old.get("text_form");
    let after = new.get("text_form");
    if before == after {
        return Vec::new();
    }
    let lost_file = file_supplyable(old) && !file_supplyable(new);
    vec![Change {
        severity: if lost_file {
            Severity::Major
        } else {
            Severity::Minor
        },
        area,
        kind: Kind::Changed,
        subject: subject.to_owned(),
        field: Some("text_form".to_owned()),
        old: before.cloned(),
        new: after.cloned(),
        message: format!(
            "{subject}: text_form {} -> {}{}",
            short(before),
            short(after),
            if lost_file {
                "; it can no longer be supplied by a mounted file, because a file delivers text \
                 and the loader does not coerce it"
            } else {
                ""
            }
        ),
    }]
}

/// Whether a file could supply this entry, tolerating one this build could not otherwise read.
///
/// The gates are right to refuse an unknown form outright. Here a document carrying one is the
/// thing being described, so the question simply has no answer and `false` is the safe one: it
/// cannot produce a false major, only miss one.
pub(super) fn file_supplyable(entry: &Json) -> bool {
    entry.get("text_form").and_then(Json::as_str) == Some("text")
}

/// The default and the parsed default are one fact: the text, and that text read.
///
/// Compared as a pair because they move together, and reporting both is one change printed twice. A
/// moved default is minor rather than patch: a chart that deliberately writes nothing for a key is
/// relying on the value the image chooses, and the value it chooses just changed.
pub(super) fn diff_default(area: Area, subject: &str, old: &Json, new: &Json) -> Vec<Change> {
    let before: Vec<Option<&Json>> = DEFAULT_FIELDS.iter().map(|name| old.get(name)).collect();
    let after: Vec<Option<&Json>> = DEFAULT_FIELDS.iter().map(|name| new.get(name)).collect();
    if before == after {
        return Vec::new();
    }
    let pair = |held: &Json| -> Json {
        let mut fields = Map::new();
        for name in DEFAULT_FIELDS {
            fields.insert(
                (name).to_owned(),
                held.get(name).cloned().unwrap_or(Json::Null),
            );
        }
        Json::Object(fields)
    };
    vec![Change {
        severity: Severity::Minor,
        area,
        kind: Kind::Changed,
        subject: subject.to_owned(),
        field: Some("default".to_owned()),
        old: Some(pair(old)),
        new: Some(pair(new)),
        message: format!(
            "{subject}: default {} -> {}{}",
            short(before[0]),
            short(after[0]),
            if before[0] == after[0] {
                format!(
                    " (text unchanged; parsed {} -> {})",
                    short(before[1]),
                    short(after[1])
                )
            } else {
                String::new()
            }
        ),
    }]
}
