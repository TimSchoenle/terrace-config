//! What changed between two versions of a contract, and what it costs the chart that reads it.
//!
//! An automated bump repins a digest, something refreshes the vendored document into the same
//! branch, and the reviewer is handed several hundred lines of reordered JSON. The question they
//! actually have — did the application gain a setting the chart has to write, drop one the chart
//! still writes, or move a default out from under it — is answerable from those bytes and nobody
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
//! removed *key* falls to step 4 of the classification — inside the prefix and spelling nothing —
//! which sits above both external lists, so nothing can absorb it and it is always major. A removed
//! *external* entry falls past step 4 to the ignore patterns, so it is major only when no surviving
//! pattern catches it, and that downgrade is decided by running the pattern rule rather than by a
//! second opinion about the language.
//!
//! **Rename detection is deliberately absent.** A renamed key arrives here as one removal and one
//! addition — a major finding and a minor one, for a change that is neither. The producer's alias
//! lists are what would collapse the pair, and a rename detector written before a producer
//! populates them would be untestable against a real document. The model is shaped so it becomes a
//! small addition rather than a rewrite: [`Kind`] reserves [`Kind::Renamed`] beside the other two,
//! and the alias fields are already compared.

use serde_json::{Map, Value as Json};

use crate::classify::matches_ignore;
use crate::document::SCHEMA_VERSION;
use crate::text::{quoted, short};

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
}

impl Area {
    /// The spelling a report uses.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Envelope => "envelope",
            Self::Loader => "loader",
            Self::Key => "key",
            Self::External => "external",
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
/// The first five fields are the machine-readable half — enough for a comment on a pull request to
/// group findings without parsing English — and `message` is the sentence a reviewer reads. `old`
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

// ------------------------------------------------------------------------------------------
// The chart's own version
// ------------------------------------------------------------------------------------------

/// The `major.minor.patch` of a chart version, or [`None`] if it is not one.
///
/// Deliberately not a full semantic-version parser: a chart version is three integers, and
/// returning [`None`] rather than guessing keeps the suggestion honest — a version this cannot read
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

// ------------------------------------------------------------------------------------------
// Comparing one contract
// ------------------------------------------------------------------------------------------

/// Per-field severity for a key that exists on both sides.
///
/// Three groups, and the reasoning for each is the reasoning the gates apply at deployment time:
///
/// - **major** — the chart's own spelling of the setting stopped being the one the image reads. A
///   spelling change should be unreachable without the path or the dialect moving, and if it
///   happens anyway it is exactly as severe as a dialect change.
/// - **minor** — the value the chart writes is still spelled right but may no longer be accepted,
///   or a default moved out from under a chart that relied on it.
/// - **patch** — prose. Real, and not a reason to bump anything.
///
/// `required` and `text_form` are absent because neither has a fixed severity; both are graded by
/// direction.
const KEY_FIELDS: [(&str, Severity); 15] = [
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
/// A smaller entry, and its spellings are its own name rather than something derived — so there is
/// no equivalent of the major group here.
const EXTERNAL_FIELDS: [(&str, Severity); 7] = [
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
const DEFAULT_FIELDS: [&str; 2] = ["default", "default_value"];

/// Compare two vendored documents — the provenance envelope, not the contract alone.
///
/// Either side may be absent: a contract that did not exist at the comparison revision, or one the
/// working tree no longer carries. Both are ordinary outcomes of a refresh rather than errors, and
/// both are graded — gaining coverage is minor, losing it is major.
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

/// One field of the provenance envelope the chart repository writes.
fn source(document: Option<&Json>, name: &str) -> Option<String> {
    document?
        .get("source")?
        .get(name)?
        .as_str()
        .map(str::to_owned)
}

/// The published document inside the provenance envelope.
fn contract_of(document: Option<&Json>) -> Map<String, Json> {
    document
        .and_then(|held| held.get("contract"))
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default()
}

/// The provenance envelope, not the published document.
///
/// The fetch timestamp is not compared: whatever refreshes a contract refuses to rewrite a file
/// whose only difference is that timestamp, so one that moved is always accompanied by something
/// that matters. The hash is not compared either — it is over the published bytes, so it is true
/// exactly when something below is, and it names nothing.
fn diff_source(old: Option<&Json>, new: Option<&Json>) -> Vec<Change> {
    let mut changes = Vec::new();
    let (before, after) = (source(old, "image"), source(new, "image"));
    if before != after {
        changes.push(Change {
            severity: Severity::Minor,
            area: Area::Envelope,
            kind: Kind::Changed,
            subject: "source.image".to_owned(),
            field: Some("image".to_owned()),
            old: before.clone().map(Json::String),
            new: after.clone().map(Json::String),
            message: format!(
                "the chart pins a different image: {} -> {}",
                before.unwrap_or_default(),
                after.unwrap_or_default()
            ),
        });
    }
    let (before, after) = (source(old, "digest"), source(new, "digest"));
    if before != after {
        changes.push(Change {
            severity: Severity::Patch,
            area: Area::Envelope,
            kind: Kind::Changed,
            subject: "source.digest".to_owned(),
            field: Some("digest".to_owned()),
            old: before.clone().map(Json::String),
            new: after.clone().map(Json::String),
            message: format!(
                "image digest {} -> {}",
                abbreviated(before.as_deref()),
                abbreviated(after.as_deref())
            ),
        });
    }
    changes
}

/// How severe a schema version move is, graded by direction rather than by shape.
///
/// A rise to a version this build implements is additive by the producer's own statement: nothing
/// was removed and nothing changed meaning, so every key a chart already writes is spelt and
/// checked exactly as before and what arrives is a constraint that says more. Worth a line — a
/// container key that gains an element schema is the signal to stop transcribing it by hand.
///
/// Everything else is major. A version *above* what this build implements is one whose keywords it
/// would walk past, and going backwards means the producer withdrew something.
fn version_severity(old: Option<&Json>, new: Option<&Json>) -> Severity {
    let (Some(before), Some(after)) = (old.and_then(Json::as_u64), new.and_then(Json::as_u64))
    else {
        return Severity::Major;
    };
    if before < after && after <= u64::from(SCHEMA_VERSION) {
        Severity::Minor
    } else {
        Severity::Major
    }
}

/// The published document's own identity: its version, the application, and the dialect.
fn diff_envelope(old: &Map<String, Json>, new: &Map<String, Json>) -> Vec<Change> {
    let mut changes = Vec::new();

    if old.get("terrace_contract") != new.get("terrace_contract") {
        changes.push(Change {
            severity: Severity::Major,
            area: Area::Envelope,
            kind: Kind::Changed,
            subject: "terrace_contract".to_owned(),
            field: None,
            old: old.get("terrace_contract").cloned(),
            new: new.get("terrace_contract").cloned(),
            message: format!(
                "envelope version {} -> {}; this build reads {}, so every rule reading this \
                 document is affected",
                short(old.get("terrace_contract")),
                short(new.get("terrace_contract")),
                crate::document::CONTRACT_VERSION
            ),
        });
    }

    let old_app = nested(old, "app");
    let new_app = nested(new, "app");
    for (name, severity) in [
        ("name", Severity::Minor),
        ("version", Severity::Patch),
        ("source", Severity::Patch),
    ] {
        if old_app.get(name) != new_app.get(name) {
            changes.push(Change {
                severity,
                area: Area::Envelope,
                kind: Kind::Changed,
                subject: format!("app.{name}"),
                field: Some(name.to_owned()),
                old: old_app.get(name).cloned(),
                new: new_app.get(name).cloned(),
                message: format!(
                    "app.{name} {} -> {}",
                    short(old_app.get(name)),
                    short(new_app.get(name))
                ),
            });
        }
    }

    let old_schema = nested(old, "schema");
    let new_schema = nested(new, "schema");
    let before = old_schema.get("schema_version");
    let after = new_schema.get("schema_version");
    if before != after {
        let severity = version_severity(before, after);
        changes.push(Change {
            severity,
            area: Area::Envelope,
            kind: Kind::Changed,
            subject: "schema.schema_version".to_owned(),
            field: None,
            old: before.cloned(),
            new: after.cloned(),
            message: format!(
                "schema version {} -> {}{}",
                short(before),
                short(after),
                if severity == Severity::Minor {
                    ", a widening this build reads"
                } else {
                    ""
                }
            ),
        });
    }

    changes.extend(diff_dialect(&old_schema, &new_schema));
    changes.extend(diff_policy(old, new));

    changes
}

/// The spellings every name the chart renders is built from.
fn diff_dialect(old_schema: &Map<String, Json>, new_schema: &Map<String, Json>) -> Vec<Change> {
    let mut changes = Vec::new();
    // The single most severe thing this can find. The prefix, the nesting separator and the
    // indirection suffix are what every environment variable name and every secret file name the
    // chart writes is built from, so one character moving here renames all of them at once — and
    // every one of the old names lands on step 4 of the classification, which a `reject` policy
    // turns into a pod that does not start. Nothing in the rendered manifests looks different.
    let old_dialect = nested(old_schema, "dialect");
    let new_dialect = nested(new_schema, "dialect");
    for name in ["prefix", "nesting_separator", "indirection_suffix"] {
        if old_dialect.get(name) != new_dialect.get(name) {
            changes.push(Change {
                severity: Severity::Major,
                area: Area::Envelope,
                kind: Kind::Changed,
                subject: format!("dialect.{name}"),
                field: Some(name.to_owned()),
                old: old_dialect.get(name).cloned(),
                new: new_dialect.get(name).cloned(),
                message: format!(
                    "dialect {name} {} -> {}; every environment and secret-file spelling this \
                     chart writes is derived from it, so all of them change at once and none of \
                     them renders differently",
                    short(old_dialect.get(name)),
                    short(new_dialect.get(name))
                ),
            });
        }
    }
    changes
}

/// What the image does with a variable no key claims.
fn diff_policy(old: &Map<String, Json>, new: &Map<String, Json>) -> Vec<Change> {
    let mut changes = Vec::new();
    let old_external = nested(old, "external");
    let new_external = nested(new, "external");
    if old_external.get("unknown") != new_external.get("unknown") {
        // Tightening is what breaks a running deployment: a variable the image used to tolerate now
        // stops it from booting. Relaxing can only turn a rejection into acceptance.
        let tightened = new_external.get("unknown").and_then(Json::as_str) == Some("reject");
        changes.push(Change {
            severity: if tightened {
                Severity::Major
            } else {
                Severity::Minor
            },
            area: Area::Envelope,
            kind: Kind::Changed,
            subject: "external.unknown".to_owned(),
            field: None,
            old: old_external.get("unknown").cloned(),
            new: new_external.get("unknown").cloned(),
            message: format!(
                "unknown-variable policy {} -> {}",
                short(old_external.get("unknown")),
                short(new_external.get("unknown"))
            ),
        });
    }

    for (kind, name) in list_delta(old_external.get("ignore"), new_external.get("ignore")) {
        changes.push(Change {
            severity: Severity::Minor,
            area: Area::Envelope,
            kind,
            subject: format!("ignore {name}"),
            field: Some("ignore".to_owned()),
            old: (kind == Kind::Removed).then(|| Json::String(name.clone())),
            new: (kind == Kind::Added).then(|| Json::String(name.clone())),
            message: format!("ignore pattern {} {}", quoted(&name), kind.label()),
        });
    }
    changes
}

/// The variables that tell the loader where its layers are.
///
/// These are the variables the chart sets to point the image at its rendered document and its
/// mounted secrets. A renamed or removed one is not a validation failure anywhere — the chart keeps
/// setting the old name, the pod keeps starting, and the application silently loads none of the
/// configuration the chart mounted. That is why a removal here is graded with a dialect change
/// rather than with a key change.
fn diff_loader(old: &Map<String, Json>, new: &Map<String, Json>) -> Vec<Change> {
    let old_entries = indexed(old.get("schema"), "loader", "env");
    let new_entries = indexed(new.get("schema"), "loader", "env");
    let mut changes = Vec::new();

    for (name, entry) in &new_entries {
        if old_entries.contains_key(name) {
            continue;
        }
        changes.push(Change {
            severity: Severity::Minor,
            area: Area::Loader,
            kind: Kind::Added,
            subject: name.clone(),
            field: None,
            old: None,
            new: Some(entry.clone()),
            message: format!(
                "loader variable {name} added (role {})",
                short(entry.get("role"))
            ),
        });
    }
    for (name, entry) in &old_entries {
        if new_entries.contains_key(name) {
            continue;
        }
        changes.push(Change {
            severity: Severity::Major,
            area: Area::Loader,
            kind: Kind::Removed,
            subject: name.clone(),
            field: None,
            old: Some(entry.clone()),
            new: None,
            message: format!(
                "loader variable {name} (role {}) is gone; a chart still setting it points the \
                 image at a layer it no longer reads, and nothing fails",
                short(entry.get("role"))
            ),
        });
    }
    for (name, before) in &old_entries {
        let Some(after) = new_entries.get(name) else {
            continue;
        };
        for (field, severity) in [
            ("role", Severity::Major),
            ("default", Severity::Minor),
            ("docs", Severity::Patch),
        ] {
            if before.get(field) != after.get(field) {
                changes.push(Change {
                    severity,
                    area: Area::Loader,
                    kind: Kind::Changed,
                    subject: name.clone(),
                    field: Some(field.to_owned()),
                    old: before.get(field).cloned(),
                    new: after.get(field).cloned(),
                    message: format!(
                        "loader variable {name}: {field} {} -> {}",
                        short(before.get(field)),
                        short(after.get(field))
                    ),
                });
            }
        }
    }
    changes
}

/// The key declarations — the half of the document a chart's rendered settings answer to.
fn diff_keys(old: &Map<String, Json>, new: &Map<String, Json>) -> Vec<Change> {
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
        // classification stops at step 4 — above both external lists — and a `reject` policy turns
        // it into a container that does not start.
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
fn diff_external(old: &Map<String, Json>, new: &Map<String, Json>) -> Vec<Change> {
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
fn diff_entry(
    area: Area,
    subject: &str,
    old: &Json,
    new: &Json,
    fields: &[(&str, Severity)],
) -> Vec<Change> {
    let mut changes = Vec::new();

    for (name, severity) in fields {
        if old.get(name) != new.get(name) {
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
    changes.extend(diff_text_form(area, subject, old, new));
    changes.extend(diff_default(area, subject, old, new));

    // A field the producer added that this has no opinion about. Reported at patch so it is visible
    // without claiming a severity nobody has reasoned about — a diff that silently ignores what it
    // does not recognise is the failure this whole toolchain exists to remove.
    let mut known: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
    known.extend(DEFAULT_FIELDS);
    known.extend(["path", "name", "required", "text_form", "unreachable"]);
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
fn diff_required(area: Area, subject: &str, old: &Json, new: &Json) -> Vec<Change> {
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

/// `text_form` decides the read, and with it whether a file can supply the setting at all.
///
/// The escalation is the point. A file delivers its contents as a string with no parse, so only a
/// `text` key can be supplied by one. A key that moves from `text` to `integer` is still spelled the
/// same and still renders the same, and the secret the chart mounts for it stops loading. Graded
/// flat that would be minor, which is wrong for exactly this case.
fn diff_text_form(area: Area, subject: &str, old: &Json, new: &Json) -> Vec<Change> {
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
fn file_supplyable(entry: &Json) -> bool {
    entry.get("text_form").and_then(Json::as_str) == Some("text")
}

/// The default and the parsed default are one fact — the text, and that text read.
///
/// Compared as a pair because they move together, and reporting both is one change printed twice. A
/// moved default is minor rather than patch: a chart that deliberately writes nothing for a key is
/// relying on the value the image chooses, and the value it chooses just changed.
fn diff_default(area: Area, subject: &str, old: &Json, new: &Json) -> Vec<Change> {
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

// ------------------------------------------------------------------------------------------
// Small shared helpers
// ------------------------------------------------------------------------------------------

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
mod tests {
    use serde_json::{Value as Json, json};

    use super::{
        Area, Kind, Severity, Status, diff_contract, observed_bump, parse_version, suggest_version,
        worst,
    };

    /// One vendored document: the provenance envelope a chart repository writes, and the contract.
    fn vendored(contract: &Json) -> Json {
        json!({
            "source": {
                "image": "ghcr.io/x/y",
                "digest": "sha256:00",
                "sha256": "ab",
                "fetched": "2026-01-01T00:00:00Z",
            },
            "contract": contract.clone(),
        })
    }

    /// One published contract, with the halves a test wants to move.
    fn document(keys: &Json, external: &Json) -> Json {
        json!({
            "terrace_contract": 1,
            "producer": {"name": "x", "version": "1", "loader": "figment"},
            "app": {"name": "x", "version": "1.0.0"},
            "schema": {
                "schema_version": 2,
                "dialect": {
                    "prefix": "P_",
                    "nesting_separator": "__",
                    "indirection_suffix": "_FILE",
                },
                "loader": [],
                "keys": keys.clone(),
            },
            "json_schema": {},
            "external": {"env": external.clone(), "ignore": [], "unknown": "reject"},
        })
    }

    /// One key declaration, with whatever a test wants to say about it.
    fn key(path: &str, extra: &Json) -> Json {
        let mut held = json!({
            "path": path,
            "env": format!("P_{}", path.to_uppercase().replace('.', "__")),
            "text_form": "text",
            "required": false,
        });
        for (name, value) in extra.as_object().expect("an object") {
            held[name] = value.clone();
        }
        held
    }

    /// A key on its own, which is what most of these compare.
    fn one(path: &str, extra: &Json) -> Json {
        json!([key(path, extra)])
    }

    /// The empty half.
    fn none() -> Json {
        json!([])
    }

    fn changes(old: &Json, new: &Json) -> Vec<super::Change> {
        diff_contract("c", "app", "p", Some(&vendored(old)), Some(&vendored(new))).changes
    }

    #[test]
    fn an_absent_side_is_an_ordinary_outcome_and_is_graded() {
        let new = vendored(&document(&none(), &none()));
        let gained = diff_contract("c", "app", "p", None, Some(&new));
        assert_eq!(gained.status, Status::Added);
        assert_eq!(gained.impact(), Severity::Minor);

        let lost = diff_contract("c", "app", "p", Some(&new), None);
        assert_eq!(lost.status, Status::Removed);
        assert_eq!(lost.impact(), Severity::Major);
    }

    #[test]
    fn a_removed_key_is_always_major_because_nothing_can_absorb_it() {
        let found = changes(
            &document(&one("a.b", &json!({})), &none()),
            &document(&none(), &none()),
        );
        let removal = found
            .iter()
            .find(|change| change.kind == Kind::Removed && change.area == Area::Key)
            .expect("the key is gone");
        assert_eq!(removal.severity, Severity::Major);
        assert!(removal.message.contains("refused at boot"), "{removal:?}");
    }

    #[test]
    fn a_removed_external_is_graded_by_what_the_classification_does_next() {
        // With a surviving pattern absorbing it, the removal costs nothing.
        let mut before = document(&none(), &json!([{"name": "RUST_LOG", "text_form": "text"}]));
        before["external"]["ignore"] = json!(["RUST_*"]);
        let mut after = document(&none(), &none());
        after["external"]["ignore"] = json!(["RUST_*"]);
        let found = changes(&before, &after);
        let removal = found
            .iter()
            .find(|change| change.area == Area::External)
            .expect("the variable is gone");
        assert_eq!(removal.severity, Severity::Minor);
        assert!(removal.message.contains("still absorbs it"), "{removal:?}");

        // With none, a chart still setting it stops booting.
        let found = changes(
            &document(&none(), &json!([{"name": "RUST_LOG", "text_form": "text"}])),
            &document(&none(), &none()),
        );
        let removal = found
            .iter()
            .find(|change| change.area == Area::External)
            .expect("the variable is gone");
        assert_eq!(removal.severity, Severity::Major);
    }

    #[test]
    fn an_added_key_is_minor_unless_it_is_required() {
        let optional = changes(
            &document(&none(), &none()),
            &document(&one("a.b", &json!({})), &none()),
        );
        assert_eq!(optional[0].severity, Severity::Minor);

        let required = changes(
            &document(&none(), &none()),
            &document(&one("a.b", &json!({"required": true})), &none()),
        );
        assert_eq!(required[0].severity, Severity::Major);
        assert!(
            required[0].message.contains("will not start"),
            "{required:?}"
        );
    }

    #[test]
    fn a_dialect_change_is_the_most_severe_thing_here() {
        let mut after = document(&none(), &none());
        after["schema"]["dialect"]["prefix"] = json!("Q_");
        let found = changes(&document(&none(), &none()), &after);
        assert_eq!(found[0].severity, Severity::Major);
        assert!(
            found[0]
                .message
                .contains("none of them renders differently"),
            "{found:?}"
        );
    }

    #[test]
    fn a_key_that_stops_being_file_supplyable_is_major_rather_than_minor() {
        // Still spelled the same, still rendered the same, and the mounted secret stops loading.
        let found = changes(
            &document(&one("a.b", &json!({"text_form": "text"})), &none()),
            &document(&one("a.b", &json!({"text_form": "integer"})), &none()),
        );
        let moved = found
            .iter()
            .find(|change| change.field.as_deref() == Some("text_form"))
            .expect("the form moved");
        assert_eq!(moved.severity, Severity::Major);
        assert!(moved.message.contains("mounted file"), "{moved:?}");
    }

    #[test]
    fn becoming_required_is_major_and_ceasing_to_be_is_not() {
        let gained = changes(
            &document(&one("a.b", &json!({})), &none()),
            &document(&one("a.b", &json!({"required": true})), &none()),
        );
        assert_eq!(gained[0].severity, Severity::Major);

        let lost = changes(
            &document(&one("a.b", &json!({"required": true})), &none()),
            &document(&one("a.b", &json!({})), &none()),
        );
        assert_eq!(lost[0].severity, Severity::Minor);
    }

    #[test]
    fn a_schema_version_rise_this_build_reads_is_a_widening() {
        let mut before = document(&none(), &none());
        before["schema"]["schema_version"] = json!(1);
        let found = changes(&before, &document(&none(), &none()));
        assert_eq!(found[0].severity, Severity::Minor);
        assert!(found[0].message.contains("a widening"), "{found:?}");
    }

    #[test]
    fn a_schema_version_beyond_this_build_is_not() {
        let mut after = document(&none(), &none());
        after["schema"]["schema_version"] = json!(99);
        let found = changes(&document(&none(), &none()), &after);
        assert_eq!(found[0].severity, Severity::Major);
    }

    #[test]
    fn tightening_the_unknown_policy_breaks_a_running_deployment_and_relaxing_it_cannot() {
        let mut lenient = document(&none(), &none());
        lenient["external"]["unknown"] = json!("warn");
        assert_eq!(
            changes(&lenient, &document(&none(), &none()))[0].severity,
            Severity::Major
        );
        assert_eq!(
            changes(&document(&none(), &none()), &lenient)[0].severity,
            Severity::Minor
        );
    }

    #[test]
    fn the_default_and_its_parsed_form_are_one_finding() {
        let found = changes(
            &document(
                &one("a.b", &json!({"default": "1", "default_value": 1})),
                &none(),
            ),
            &document(
                &one("a.b", &json!({"default": "2", "default_value": 2})),
                &none(),
            ),
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].field.as_deref(), Some("default"));
        assert_eq!(found[0].severity, Severity::Minor);
    }

    #[test]
    fn a_field_this_diff_models_no_severity_for_is_reported_rather_than_dropped() {
        let found = changes(
            &document(&one("a.b", &json!({"something_later": 1})), &none()),
            &document(&one("a.b", &json!({"something_later": 2})), &none()),
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].severity, Severity::Patch);
        assert!(found[0].message.contains("models no severity"), "{found:?}");
    }

    #[test]
    fn an_unreadable_envelope_is_described_rather_than_refused() {
        // The one document this most needs to describe is the one a gate would decline to read.
        let mut after = document(&none(), &none());
        after["terrace_contract"] = json!(99);
        let found = changes(&document(&none(), &none()), &after);
        assert_eq!(found[0].severity, Severity::Major);
        assert_eq!(found[0].subject, "terrace_contract");
    }

    #[test]
    fn the_json_schema_is_only_worth_a_line_when_no_key_moved() {
        let mut after = document(&none(), &none());
        after["json_schema"] = json!({"type": "object"});
        let found = changes(&document(&none(), &none()), &after);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].subject, "json_schema");

        // With a key moving too, the schema is derived from it and saying so again would be noise.
        let mut after = document(&one("a.b", &json!({})), &none());
        after["json_schema"] = json!({"type": "object"});
        let found = changes(&document(&none(), &none()), &after);
        assert!(
            !found.iter().any(|change| change.subject == "json_schema"),
            "{found:?}"
        );
    }

    #[test]
    fn a_message_stays_one_line_when_the_prose_it_quotes_does_not() {
        // A key's documentation is paragraphs, and a finding is a line. The quoting is what keeps
        // that true, and getting it wrong is invisible until a report is read.
        let found = changes(
            &document(&one("a.b", &json!({"docs": "one"})), &none()),
            &document(&one("a.b", &json!({"docs": "one\n\ntwo"})), &none()),
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(!found[0].message.contains('\n'), "{:?}", found[0].message);
    }

    #[test]
    fn a_version_this_cannot_read_produces_no_suggestion_rather_than_a_wrong_one() {
        assert_eq!(parse_version(Some("1.2.3")), Some((1, 2, 3)));
        assert_eq!(parse_version(Some("1.2")), None);
        assert_eq!(parse_version(Some("1.2.3-rc1")), None);
        assert_eq!(suggest_version(Some("nope"), Severity::Major), None);
    }

    #[test]
    fn a_suggestion_moves_the_component_the_impact_names() {
        assert_eq!(
            suggest_version(Some("1.2.3"), Severity::Major).as_deref(),
            Some("2.0.0")
        );
        assert_eq!(
            suggest_version(Some("1.2.3"), Severity::Minor).as_deref(),
            Some("1.3.0")
        );
        assert_eq!(
            suggest_version(Some("1.2.3"), Severity::Patch).as_deref(),
            Some("1.2.4")
        );
        assert_eq!(suggest_version(Some("1.2.3"), Severity::None), None);
    }

    #[test]
    fn the_bump_already_in_the_branch_is_read_from_the_pair() {
        assert_eq!(
            observed_bump(Some("1.2.3"), Some("2.0.0")),
            Some(Severity::Major)
        );
        assert_eq!(
            observed_bump(Some("1.2.3"), Some("1.3.0")),
            Some(Severity::Minor)
        );
        assert_eq!(
            observed_bump(Some("1.2.3"), Some("1.2.3")),
            Some(Severity::None)
        );
        assert_eq!(observed_bump(None, Some("1.2.3")), None);
    }

    #[test]
    fn the_impact_is_the_largest_of_the_findings() {
        assert_eq!(
            worst([Severity::Patch, Severity::Major, Severity::Minor]),
            Severity::Major
        );
        assert_eq!(worst([]), Severity::None);
    }
}
