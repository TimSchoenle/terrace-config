//! Comparing two constraints for refinements that narrow what a value may be.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::document::{Tightening, entry_name_pattern_of, required_entries_of, tightenings};

use super::{Area, Change, Kind, Severity};

/// Every other tightening — a string's pattern, and anything inside the key — graded by direction.
///
/// The key's own required entries and entry-name pattern have their own graders above, with their
/// own wording. Everything else is compared position by position and keyword by keyword: one gained
/// or replaced is a tightening, major for the reason those are, and one lost is a relaxation.
pub(super) fn diff_tightenings(area: Area, subject: &str, old: &Json, new: &Json) -> Vec<Change> {
    let graded = |constraint: Option<&Json>| -> BTreeMap<(String, String), Json> {
        tightenings(constraint)
            .into_iter()
            .filter(|(at, tightening)| {
                !at.is_empty() || matches!(tightening, Tightening::Matches(_))
            })
            .map(|(at, tightening)| {
                let value = match &tightening {
                    Tightening::Entries(entries) => json_list(entries),
                    Tightening::Names(pattern)
                    | Tightening::Matches(pattern)
                    | Tightening::Holds(pattern) => Json::String((*pattern).to_owned()),
                };
                // Conditions are many to a position, so each is its own entry, named by what it says.
                let keyword = match &tightening {
                    Tightening::Holds(description) => format!("allOf {description}"),
                    other => other.keyword().to_owned(),
                };
                ((at, keyword), value)
            })
            .collect()
    };
    let before = graded(old.get("constraint"));
    let after = graded(new.get("constraint"));

    let mut changes = Vec::new();
    for ((at, keyword), value) in &after {
        let held = before.get(&(at.clone(), keyword.clone()));
        if held == Some(value) {
            continue;
        }
        changes.push(Change {
            severity: Severity::Major,
            area,
            kind: Kind::Changed,
            subject: subject.to_owned(),
            field: Some(tightening_field(at, keyword)),
            old: held.cloned(),
            new: Some(value.clone()),
            message: format!(
                "{subject}{} now {}; a value that does not will stop the image starting",
                position_of(at),
                phrase(keyword, value)
            ),
        });
    }
    for ((at, keyword), value) in &before {
        if after.contains_key(&(at.clone(), keyword.clone())) {
            continue;
        }
        changes.push(Change {
            severity: Severity::Minor,
            area,
            kind: Kind::Changed,
            subject: subject.to_owned(),
            field: Some(tightening_field(at, keyword)),
            old: Some(value.clone()),
            new: None,
            message: format!(
                "{subject}{} no longer {}",
                position_of(at),
                phrase(keyword, value)
            ),
        });
    }
    changes
}

pub(super) fn json_list(names: &[&str]) -> Json {
    Json::Array(
        names
            .iter()
            .map(|name| Json::String((*name).to_owned()))
            .collect(),
    )
}

pub(super) fn tightening_field(at: &str, keyword: &str) -> String {
    let keyword = keyword.split(' ').next().unwrap_or(keyword);
    if at.is_empty() {
        format!("constraint.{keyword}")
    } else {
        format!("constraint.{at}.{keyword}")
    }
}

pub(super) fn position_of(at: &str) -> String {
    if at.is_empty() {
        String::new()
    } else {
        format!(" at `{at}`")
    }
}

/// What one tightening requires, as a verb phrase after "now" or "no longer".
pub(super) fn phrase(keyword: &str, value: &Json) -> String {
    let text = |value: &Json| {
        value.as_array().map_or_else(
            || value.as_str().unwrap_or_default().to_owned(),
            |names| {
                names
                    .iter()
                    .filter_map(Json::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        )
    };
    match keyword {
        "required" => format!("has to contain {}", text(value)),
        "propertyNames" => format!("holds its entry names to {}", text(value)),
        "pattern" => format!("has to match {}", text(value)),
        _ => format!("has to satisfy: {}", text(value)),
    }
}

/// A constraint with every tightening taken out, for comparing everything else about it. Each is
/// graded by direction above.
pub(super) fn without_refinements(constraint: Option<&Json>) -> Option<Json> {
    let mut held = constraint.cloned()?;
    strip_tightenings(&mut held, 0);
    Some(held)
}

/// [`tightenings`], removed: the walk it takes, taking out what it finds.
pub(super) fn strip_tightenings(schema: &mut Json, depth: usize) {
    let Some(fields) = schema.as_object_mut() else {
        return;
    };
    if depth > 32 {
        return;
    }
    if !required_entries_of(fields).is_empty() {
        fields.remove("required");
    }
    if entry_name_pattern_of(fields).is_some() {
        fields.remove("propertyNames");
    }
    if fields.get("pattern").is_some_and(Json::is_string) {
        fields.remove("pattern");
    }
    if let Some(Json::Array(members)) = fields.get_mut("allOf") {
        members.retain(|member| member.get("description").is_none());
        if members.is_empty() {
            fields.remove("allOf");
        }
    }
    for keyword in ["additionalProperties", "items"] {
        if let Some(element) = fields.get_mut(keyword) {
            strip_tightenings(element, depth + 1);
        }
    }
    if let Some(Json::Object(properties)) = fields.get_mut("properties") {
        for field in properties.values_mut() {
            strip_tightenings(field, depth + 1);
        }
    }
}
