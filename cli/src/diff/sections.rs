//! Comparing the document's sections other than its keys: the source, the envelope, the dialect,
//! the policy, the loader and the reload declaration.

use serde_json::{Map, Value as Json};

use crate::document::SCHEMA_VERSION;
use crate::text::{quoted, short};

use super::{Area, Change, Kind, Severity, abbreviated, indexed, list_delta, nested};

/// One field of the provenance envelope the chart repository writes.
pub(super) fn source(document: Option<&Json>, name: &str) -> Option<String> {
    document?
        .get("source")?
        .get(name)?
        .as_str()
        .map(str::to_owned)
}

/// The published document inside the provenance envelope.
pub(super) fn contract_of(document: Option<&Json>) -> Map<String, Json> {
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
pub(super) fn diff_source(old: Option<&Json>, new: Option<&Json>) -> Vec<Change> {
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
pub(super) fn version_severity(old: Option<&Json>, new: Option<&Json>) -> Severity {
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
pub(super) fn diff_envelope(old: &Map<String, Json>, new: &Map<String, Json>) -> Vec<Change> {
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
pub(super) fn diff_dialect(
    old_schema: &Map<String, Json>,
    new_schema: &Map<String, Json>,
) -> Vec<Change> {
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
pub(super) fn diff_policy(old: &Map<String, Json>, new: &Map<String, Json>) -> Vec<Change> {
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
pub(super) fn diff_loader(old: &Map<String, Json>, new: &Map<String, Json>) -> Vec<Change> {
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

/// What moved in whether a change reaches the running process.
///
/// **Graded by the effective class, not by the bytes**, for the reason removals are graded by what
/// the loader does next: the same deployment behaviour has more than one spelling. A key going from
/// undeclared to `restart` is the same restart it always was, and an image declaring `none` where
/// it declared nothing before rolls the same pods. Neither is a finding.
///
/// What is a finding is a key whose class a consumer derives differently — a chart's generated
/// restart set has to follow it, and the gates fail until it does — and a change to the image's
/// own declaration, which can move every key at once. Both are minor: the chart's values still
/// load either way, and what changes is which edits roll its pods.
pub(super) fn diff_reload(old: &Map<String, Json>, new: &Map<String, Json>) -> Vec<Change> {
    let old_schema = nested(old, "schema");
    let new_schema = nested(new, "schema");
    let mut changes = Vec::new();

    let old_declared = old_schema.get("reload").filter(|held| !held.is_null());
    let new_declared = new_schema.get("reload").filter(|held| !held.is_null());
    let old_rebuilds = rebuilds(old_declared);
    let new_rebuilds = rebuilds(new_declared);
    if old_rebuilds != new_rebuilds || (old_rebuilds && old_declared != new_declared) {
        changes.push(Change {
            severity: Severity::Minor,
            area: Area::Reload,
            kind: Kind::Changed,
            subject: "schema.reload".to_owned(),
            field: None,
            old: old_declared.cloned(),
            new: new_declared.cloned(),
            message: match (old_rebuilds, new_rebuilds) {
                (false, true) => "the image now applies a change without restarting; keys it \
                                  publishes `live` stop rolling the pods once the chart \
                                  regenerates its restart set"
                    .to_owned(),
                (true, false) => "the image no longer applies a change without restarting; every \
                                  key rolls the pods again, and the chart's restart set has to \
                                  follow"
                    .to_owned(),
                _ => format!(
                    "the image changed what it watches: {} -> {}",
                    short(old_declared),
                    short(new_declared)
                ),
            },
        });
    }

    let old_keys = indexed(old.get("schema"), "keys", "path");
    let new_keys = indexed(new.get("schema"), "keys", "path");
    for (path, before) in &old_keys {
        let Some(after) = new_keys.get(path) else {
            continue;
        };
        let was_live = old_rebuilds && live(before);
        let is_live = new_rebuilds && live(after);
        // Only a key whose own class moved. One that moved because the declaration did is covered
        // by the finding above, and listing every key again would bury it.
        if was_live == is_live || (old_rebuilds != new_rebuilds && live(before) == live(after)) {
            continue;
        }
        changes.push(Change {
            severity: Severity::Minor,
            area: Area::Reload,
            kind: Kind::Changed,
            subject: path.clone(),
            field: Some("reload".to_owned()),
            old: before.get("reload").cloned(),
            new: after.get("reload").cloned(),
            message: if is_live {
                format!(
                    "{path} is now applied by a rebuild; a change to it no longer needs to roll the \
                     pods, and gets no readiness gating or rollback when it does not"
                )
            } else {
                format!(
                    "{path} now needs a restart; a chart that leaves it out of its restart set \
                     reports a change to it as applied and never applies it"
                )
            },
        });
    }
    changes
}

/// Whether a raw `schema.reload` declares a rebuild over at least one layer.
pub(super) fn rebuilds(declared: Option<&Json>) -> bool {
    declared.is_some_and(|held| {
        held.get("mode").and_then(Json::as_str) == Some("rebuild")
            && held
                .get("layers")
                .and_then(Json::as_array)
                .is_some_and(|layers| !layers.is_empty())
    })
}

/// Whether a raw key is published `live`. Anything else — absent, `restart`, a class this build
/// does not know — is a restart.
pub(super) fn live(entry: &Json) -> bool {
    entry.get("reload").and_then(Json::as_str) == Some("live")
}
