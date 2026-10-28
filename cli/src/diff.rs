//! What changed between two versions of a contract, and what it costs the chart that reads it.
//!
//! An automated bump repins a digest, something refreshes the vendored document into the same
//! branch, and the reviewer is handed several hundred lines of reordered JSON. The question they
//! actually have (did the application gain a setting the chart has to write, drop one the chart
//! still writes, or move a default out from under it) is answerable from those bytes and nobody
//! answers it. This turns the two documents into that answer, and into a defensible suggestion
//! about the chart's own version number.
//!
//! Pure and offline: two parsed documents in, findings out. Reading the old bytes out of version
//! control and walking a chart tree belong one feature up, which is what lets every rule here be
//! tested by calling it.
//!
//! # Four decisions that are deliberate rather than incidental
//!
//! **It does not gate on the envelope.** Every other consumer refuses a document whose
//! `terrace_contract` it does not recognise, which is right for a gate: misreading a document is
//! worse than not reading it. It is wrong here, because "the envelope version changed" is one of
//! the findings this exists to report, and a tool that fell over on the one document it most needs
//! to describe is useless on the day it matters. So both sides are read defensively, field by
//! field, and an envelope nothing could validate is reported as the major change it is.
//!
//! **The severity table is a suggestion with its reasons attached, never an edit.** Each finding
//! maps to the smallest chart version bump it justifies, and the chart's impact is the largest of
//! them. Nothing here writes a chart's version: the mapping is a defensible default and the
//! reviewer is the one who knows whether the chart writes the key that moved. A tool that edited
//! the version would be asserting it knows that, and would be wrong the first time a removed key
//! was one no template ever emitted.
//!
//! **Removing something is graded by what the loader does next, not by the shape of the diff.** A
//! removed *key* falls to step 4 of the classification (inside the prefix and spelling nothing),
//! which sits above both external lists, so nothing can absorb it and it is always major. A removed
//! *external* entry falls past step 4 to the ignore patterns, so it is major only when no surviving
//! pattern catches it, and that downgrade is decided by running the pattern rule rather than by a
//! second opinion about the language.
//!
//! **Rename detection is deliberately absent.** A renamed key arrives here as one removal and one
//! addition: a major finding and a minor one, for a change that is neither. The producer's alias
//! lists are what would collapse the pair, and a rename detector written before a producer
//! populates them would be untestable against a real document. The model is shaped so it becomes a
//! small addition rather than a rewrite: [`Kind`] reserves [`Kind::Renamed`] beside the other two,
//! and the alias fields are already compared.

mod keys;
mod sections;
mod tightening;

use serde_json::{Map, Value as Json};

use crate::text::short;

use self::keys::{diff_external, diff_keys};
use self::sections::{contract_of, diff_envelope, diff_loader, diff_reload, diff_source, source};

/// The smallest chart version bump a finding justifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Nothing moved.
    None,
    /// Prose. Real, and not a reason to bump anything.
    Patch,
    /// The value the chart writes may no longer be accepted, or a default moved out from under it.
    Minor,
    /// The chart's own spelling of a setting stopped being the one the image reads, or a
    /// deployment that works today stops working.
    Major,
}

impl Severity {
    /// The spelling a report uses.
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Patch => "patch",
            Self::Minor => "minor",
            Self::Major => "major",
        }
    }
}

/// The largest of a run of severities.
pub fn worst(severities: impl IntoIterator<Item = Severity>) -> Severity {
    severities.into_iter().max().unwrap_or(Severity::None)
}

/// Which half of the document a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    /// The document's own identity: its version, the application, the dialect, the policy.
    Envelope,
    /// A variable the loader reads to decide what the layers are.
    Loader,
    /// A key declaration.
    Key,
    /// A declared external variable.
    External,
    /// Whether a change reaches the running process: `schema.reload`, and each key's class.
    Reload,
}

impl Area {
    /// The spelling a report uses.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Envelope => "envelope",
            Self::Loader => "loader",
            Self::Key => "key",
            Self::External => "external",
            Self::Reload => "reload",
        }
    }
}

/// What happened to the thing a finding names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// It is there now and was not.
    Added,
    /// It was there and is not.
    Removed,
    /// It is there in both, and something about it moved.
    Changed,
    /// Reserved. See the note on rename detection.
    Renamed,
}

impl Kind {
    /// The spelling a report uses.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Changed => "changed",
            Self::Renamed => "renamed",
        }
    }
}

/// One difference, and the smallest chart version bump it justifies.
///
/// The first five fields are the machine-readable half, enough for a comment on a pull request to
/// group findings without parsing English, and `message` is the sentence a reviewer reads. `old`
/// and `new` are the raw values, so a consumer that wants to render a before and after does not
/// have to recover them from the prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The smallest bump this justifies.
    pub severity: Severity,
    /// Which half of the document.
    pub area: Area,
    /// What happened.
    pub kind: Kind,
    /// What it happened to.
    pub subject: String,
    /// Which field of it, when the change is to one.
    pub field: Option<String>,
    /// What it was.
    pub old: Option<Json>,
    /// What it is.
    pub new: Option<Json>,
    /// The sentence a reviewer reads.
    pub message: String,
}

impl Change {
    /// This finding as JSON, for a consumer that is not a person.
    pub fn as_json(&self) -> Json {
        serde_json::json!({
            "severity": self.severity.label(),
            "area": self.area.label(),
            "kind": self.kind.label(),
            "subject": self.subject,
            "field": self.field,
            "old": self.old,
            "new": self.new,
            "message": self.message,
        })
    }
}

/// Whether a contract exists on both sides, and whether anything about it moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The working tree carries it and the comparison revision did not.
    Added,
    /// The comparison revision carried it and the working tree does not.
    Removed,
    /// Both, and something moved.
    Changed,
    /// Both, and nothing did.
    Unchanged,
}

impl Status {
    /// The spelling a report uses.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Changed => "changed",
            Self::Unchanged => "unchanged",
        }
    }
}

/// One vendored contract, across the two revisions.
#[derive(Debug, Clone)]
pub struct ContractDiff {
    /// The chart that carries it.
    pub chart: String,
    /// The document's name in the declaration.
    pub name: String,
    /// Where the file is.
    pub path: String,
    /// Whether it exists on both sides.
    pub status: Status,
    /// Every difference.
    pub changes: Vec<Change>,
    /// The image it was for, before.
    pub old_image: Option<String>,
    /// The image it is for, now.
    pub new_image: Option<String>,
}

impl ContractDiff {
    /// The largest severity among the changes.
    pub fn impact(&self) -> Severity {
        worst(self.changes.iter().map(|change| change.severity))
    }

    /// This contract's diff as JSON.
    pub fn as_json(&self) -> Json {
        serde_json::json!({
            "chart": self.chart,
            "name": self.name,
            "path": self.path,
            "status": self.status.label(),
            "impact": self.impact().label(),
            "image": {"old": self.old_image, "new": self.new_image},
            "changes": self.changes.iter().map(Change::as_json).collect::<Vec<_>>(),
        })
    }
}

/// The `major.minor.patch` of a chart version, or [`None`] if it is not one.
///
/// Deliberately not a full semantic-version parser: a chart version is three integers, and
/// returning [`None`] rather than guessing keeps the suggestion honest: a version this cannot read
/// produces no suggestion instead of a wrong one.
pub fn parse_version(version: Option<&str>) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = version?.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut held = [0u64; 3];
    for (index, part) in parts.iter().enumerate() {
        held[index] = part.parse().ok()?;
    }
    Some((held[0], held[1], held[2]))
}

/// The version the suggested impact would produce from the working tree's current one.
pub fn suggest_version(current: Option<&str>, impact: Severity) -> Option<String> {
    let (major, minor, patch) = parse_version(current)?;
    match impact {
        Severity::None => None,
        Severity::Major => Some(format!("{}.0.0", major + 1)),
        Severity::Minor => Some(format!("{major}.{}.0", minor + 1)),
        Severity::Patch => Some(format!("{major}.{minor}.{}", patch + 1)),
    }
}

/// Which component of the chart version already moved between the two revisions.
pub fn observed_bump(old: Option<&str>, new: Option<&str>) -> Option<Severity> {
    let before = parse_version(old)?;
    let after = parse_version(new)?;
    Some(if after.0 != before.0 {
        Severity::Major
    } else if after.1 != before.1 {
        Severity::Minor
    } else if after.2 != before.2 {
        Severity::Patch
    } else {
        Severity::None
    })
}

/// Compares two vendored documents: the provenance envelope, not the contract alone.
///
/// Either side may be absent: a contract that did not exist at the comparison revision, or one the
/// working tree no longer carries. Both are ordinary outcomes of a refresh rather than errors, and
/// both are graded: gaining coverage is minor, losing it is major.
///
/// # Panics
/// Never: two absent documents is a caller error the walk above cannot produce, and is reported as
/// an unchanged contract rather than by unwinding.
pub fn diff_contract(
    chart: &str,
    name: &str,
    path: &str,
    old: Option<&Json>,
    new: Option<&Json>,
) -> ContractDiff {
    let mut diff = ContractDiff {
        chart: chart.to_owned(),
        name: name.to_owned(),
        path: path.to_owned(),
        status: Status::Unchanged,
        changes: Vec::new(),
        old_image: source(old, "image"),
        new_image: source(new, "image"),
    };

    match (old, new) {
        (None, None) => return diff,
        (None, Some(_)) => {
            diff.status = Status::Added;
            let image = diff.new_image.clone().unwrap_or_default();
            diff.changes.push(Change {
                severity: Severity::Minor,
                area: Area::Envelope,
                kind: Kind::Added,
                subject: name.to_owned(),
                field: None,
                old: None,
                new: Some(Json::String(image.clone())),
                message: format!(
                    "contract is new: {image} publishes a document this chart did not vendor \
                     before, so its settings are gated from now on"
                ),
            });
            return diff;
        }
        (Some(_), None) => {
            diff.status = Status::Removed;
            let image = diff.old_image.clone().unwrap_or_default();
            diff.changes.push(Change {
                severity: Severity::Major,
                area: Area::Envelope,
                kind: Kind::Removed,
                subject: name.to_owned(),
                field: None,
                old: Some(Json::String(image)),
                new: None,
                message: "contract is gone: nothing validates the settings this document covered, \
                          and a chart that still writes them is no longer checked against anything"
                    .to_owned(),
            });
            return diff;
        }
        (Some(_), Some(_)) => {}
    }

    let before = contract_of(old);
    let after = contract_of(new);

    let mut changes = diff_source(old, new);
    changes.extend(diff_envelope(&before, &after));
    changes.extend(diff_loader(&before, &after));
    let key_changes = diff_keys(&before, &after);
    let keys_moved = !key_changes.is_empty();
    changes.extend(key_changes);
    changes.extend(diff_external(&before, &after));
    changes.extend(diff_reload(&before, &after));

    // The published JSON Schema is derived from the keys, so it moves whenever they do and saying
    // so again would be noise. It is worth a line in exactly one case: it moved and the keys did
    // not, which means the document changed in a way nothing above models.
    if !keys_moved && before.get("json_schema") != after.get("json_schema") {
        changes.push(Change {
            severity: Severity::Patch,
            area: Area::Envelope,
            kind: Kind::Changed,
            subject: "json_schema".to_owned(),
            field: None,
            old: None,
            new: None,
            message: "the published JSON Schema changed while no key declaration did; this diff \
                      does not model whatever moved, so read the raw document"
                .to_owned(),
        });
    }

    diff.status = if changes.is_empty() {
        Status::Unchanged
    } else {
        Status::Changed
    };
    diff.changes = changes;
    diff
}

/// One nested object, empty when it is missing or is not one.
fn nested(document: &Map<String, Json>, name: &str) -> Map<String, Json> {
    document
        .get(name)
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default()
}

/// Index one list of declarations by the field that names it, dropping what is not one.
///
/// Defensive by design: this reads documents the gates would refuse, so a section that is missing,
/// is not a list, or holds something other than named objects has to produce an empty index rather
/// than a failure. Whatever is wrong with such a document shows up as the envelope or dialect
/// finding that explains it.
fn indexed(
    section: Option<&Json>,
    name: &str,
    key: &str,
) -> std::collections::BTreeMap<String, Json> {
    section
        .and_then(|held| held.get(name))
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            entry
                .get(key)
                .and_then(Json::as_str)
                .map(|named| (named.to_owned(), entry.clone()))
        })
        .collect()
}

/// Added and removed members of a list of strings, order-insensitively.
fn list_delta(old: Option<&Json>, new: Option<&Json>) -> Vec<(Kind, String)> {
    let members = |held: Option<&Json>| -> std::collections::BTreeSet<String> {
        held.and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
            .map(str::to_owned)
            .collect()
    };
    let before = members(old);
    let after = members(new);
    let mut delta: Vec<(Kind, String)> = after
        .difference(&before)
        .map(|name| (Kind::Added, name.clone()))
        .collect();
    delta.extend(
        before
            .difference(&after)
            .map(|name| (Kind::Removed, name.clone())),
    );
    delta
}

/// A key's one-line shape, for a message that would otherwise say only its name.
fn shape_of(entry: &Json) -> String {
    let mut parts = vec![
        entry
            .get("text_form")
            .and_then(Json::as_str)
            .unwrap_or("unknown")
            .to_owned(),
    ];
    if truthy(entry.get("secret")) {
        parts.push("secret".to_owned());
    }
    match entry.get("default").filter(|held| !held.is_null()) {
        Some(held) => parts.push(format!("default {}", short(Some(held)))),
        None => parts.push("no default".to_owned()),
    }
    parts.join(", ")
}

/// One value as the text a message shows, or the empty string.
fn text_of(value: Option<&Json>) -> String {
    value
        .and_then(Json::as_str)
        .map_or_else(String::new, str::to_owned)
}

/// Whether a field is present and not false or null.
fn truthy(value: Option<&Json>) -> bool {
    value.is_some_and(|held| held.as_bool().unwrap_or(!held.is_null()))
}

/// One digest, short enough for a sentence.
fn abbreviated(digest: Option<&str>) -> String {
    match digest {
        None => "unset".to_owned(),
        Some(held) if held.chars().count() > 22 => {
            format!("{}...", held.chars().take(19).collect::<String>())
        }
        Some(held) => held.to_owned(),
    }
}

#[cfg(test)]
mod tests;
