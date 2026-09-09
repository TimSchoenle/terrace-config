//! `charts/<chart>/config-contract.yaml`, and binding it to the contracts it names.
//!
//! Two jobs, and they are the same job at two removes: reading what a chart declares, and turning
//! that into the contracts a document is actually validated against. The second is where the
//! staleness interlock sits, because refusing to validate is a decision about the *declaration* —
//! the chart pins a digest, the vendored file is for a digest, and if the two differ nothing
//! downstream has anything trustworthy to say.
//!
//! # Unknown keys are refused, and that is the opposite of how a document is read
//!
//! [`crate::document`] reads tolerantly: a producer emitting a field this build has not learned is
//! ahead rather than wrong. This file is the other case. It is written by hand, in this repository,
//! and its whole job is to be exhaustive — so a key nobody reads is a typo, and ignoring it in
//! silence is the worst available failure mode.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize as _;
use serde_json::Value as Json;

use crate::error::Error;
use crate::gate::{DocumentFormat, DocumentSource, Relaxed};
use crate::union::{Union, local_refs_only, union_contracts};

use super::{DECLARATION, dig, release, shown};

/// Keys the top level may carry.
const DECLARATION_KEYS: [&str; 6] = [
    "bindings",
    "credentials",
    "documents",
    "reason",
    "unbound",
    "unconfigured",
];
const CREDENTIAL_KEYS: [&str; 3] = ["key", "note", "value"];
const DOCUMENT_KEYS: [&str; 5] = ["consumers", "exempt", "images", "name", "source"];
const SOURCE_KEYS: [&str; 4] = ["format", "key", "kind", "selector"];
const IMAGE_KEYS: [&str; 2] = ["contract", "values"];
const CONSUMER_KEYS: [&str; 2] = ["containers", "workload"];
const EXEMPT_KEYS: [&str; 3] = ["gates", "reason", "values"];
const UNBOUND_KEYS: [&str; 3] = ["documents", "keys", "reason"];

/// Which rendered object holds a document, and where inside it.
pub type Source = DocumentSource;

/// One image a document is read by, and the vendored copy of its contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    /// The values path the chart pins the image at.
    pub values: String,
    /// The vendored contract, relative to the chart directory.
    pub contract: String,
}

/// One workload that reads a document, and the containers of it that do.
#[derive(Debug, Clone)]
pub struct Consumer {
    /// The workload's kind.
    pub kind: String,
    /// The labels naming it.
    pub selector: Vec<(String, String)>,
    /// The containers inside it that read the document.
    pub containers: Vec<String>,
}

/// One rendered values file, exempt from some gates, with a written reason.
#[derive(Debug, Clone)]
pub struct Exemption {
    /// The values file, or `*` for every one of them.
    pub values: String,
    /// The gates it relaxes.
    pub gates: Vec<String>,
    /// Why. Mandatory: an unexplained hole in a gate is indistinguishable from an oversight.
    pub reason: String,
}

/// Contract keys this chart deliberately surfaces no value for, and why.
///
/// A different axis from [`Exemption`], and the two are not interchangeable. An exemption is per
/// values file and per gate: it says one rendered fixture is not held to one of the four checks.
/// This says a set of *keys* has no chart value binding it, which is a property of the chart and of
/// every fixture at once, and it relaxes no gate.
#[derive(Debug, Clone)]
pub struct Unbound {
    /// The keys, written out one by one. There is no pattern form, so a key an image release adds
    /// cannot be written off by something somebody typed before it existed.
    pub keys: Vec<String>,
    /// Why.
    pub reason: String,
    /// The documents this is scoped to, or [`None`] for every one that declares the key.
    pub documents: Option<Vec<String>>,
}

/// What one chart says about one credential its images declare.
#[derive(Debug, Clone)]
pub struct CredentialNote {
    /// The contract key.
    pub key: String,
    /// The chart value an operator may set instead of creating the Secret.
    pub value: Option<String>,
    /// The condition under which this release needs the credential at all.
    pub note: Option<String>,
}

/// One declared configuration document.
#[derive(Debug, Clone)]
pub struct Document {
    /// The name the declaration gives it, and the one every message names it by.
    pub name: String,
    /// Which rendered object holds it.
    pub source: Source,
    /// The images that read it.
    pub images: Vec<ImageRef>,
    /// The workloads that mount it.
    pub consumers: Vec<Consumer>,
    /// The exemptions, per values file.
    pub exempt: Vec<Exemption>,
}

impl Document {
    /// The gates exempted for one rendered values file.
    ///
    /// `values: "*"` covers every one of them. That is for a gap in the *contract* rather than in
    /// one fixture — a key the chart renders and the image's document does not describe — which is
    /// a property of the pair and not of any particular values file.
    pub fn relaxed(&self, values_file: &str) -> Relaxed {
        let mut relaxed = Relaxed::default();
        let wanted = file_name(values_file);
        for exemption in &self.exempt {
            if exemption.values == "*" || file_name(&exemption.values) == wanted {
                for gate in &exemption.gates {
                    relaxed.relax(gate);
                }
            }
        }
        relaxed
    }
}

/// The last path segment, whichever separator a declaration was written with.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// One chart's `config-contract.yaml`.
#[derive(Debug, Clone)]
pub struct Declaration {
    /// The chart's directory name.
    pub chart: String,
    /// The declaration's own path, for a message.
    pub path: PathBuf,
    /// Every document the chart declares.
    pub documents: Vec<Document>,
    /// Why the chart declares none, when it declares none.
    pub reason: Option<String>,
    /// Values paths at which this chart pins an image that reads no contract-described
    /// configuration. **Values paths, not repository names** — a repository name is ambiguous where
    /// a values path is not, since a chart pinning one repository twice could not say which it
    /// meant.
    pub unconfigured: Vec<String>,
    /// Whether the chart is enrolled in the marker check.
    ///
    /// A declared fact rather than an inferred one. Enrolment used to be "the chart carries at
    /// least one marker", which reads well until a chart *stops* carrying them: a botched edit, a
    /// rewritten values file, a marker spelt in a way the parser does not recognise, and the chart
    /// drops out of the report without a word.
    pub bindings: bool,
    /// The keys this chart binds no value to, each group with a written reason.
    pub unbound: Vec<Unbound>,
    /// What this chart adds to the credential reference its README carries, keyed by contract path.
    pub credentials: BTreeMap<String, CredentialNote>,
}

/// Read one chart's declaration; [`None`] when it has none.
///
/// # Errors
/// [`Error::Invalid`] when the file exists and is not a declaration, naming the key that is wrong.
pub fn load_declaration(chart_dir: &Path) -> Result<Option<Declaration>, Error> {
    let path = chart_dir.join(DECLARATION);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| Error::io(path.display(), e))?;
    let document: Json = read_yaml(&text, &path)?;
    let at = path.display().to_string();

    let Some(fields) = document.as_object() else {
        return Err(Error::Invalid(format!(
            "{at}: expected a mapping at the top level"
        )));
    };
    reject_unknown(&at, "", &document, &DECLARATION_KEYS)?;

    let Some(raw) = fields.get("documents").and_then(Json::as_array) else {
        return Err(Error::Invalid(format!(
            "{at}: `documents` is missing or not a list"
        )));
    };

    let reason = fields
        .get("reason")
        .and_then(Json::as_str)
        .map(str::to_owned);
    if raw.is_empty() && reason.is_none() {
        return Err(Error::Invalid(format!(
            "{at}: `documents` is empty, which is an explicit opt-out and needs a `reason`"
        )));
    }

    let bindings = match fields.get("bindings") {
        None | Some(Json::Null) => false,
        Some(Json::Bool(held)) => *held,
        other => {
            return Err(Error::Invalid(format!(
                "{at}: `bindings` is the chart's enrolment in the marker check and must be true or \
                 false, not {}",
                shown(other)
            )));
        }
    };

    let mut documents = Vec::new();
    for entry in raw {
        documents.push(load_document(&at, entry)?);
    }
    let names: BTreeSet<&str> = documents.iter().map(|entry| entry.name.as_str()).collect();

    Ok(Some(Declaration {
        chart: chart_dir
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        path: path.clone(),
        reason,
        unconfigured: strings(fields.get("unconfigured")),
        bindings,
        unbound: load_unbound(&at, fields.get("unbound"), &names)?,
        credentials: load_credentials(&at, fields.get("credentials"))?,
        documents,
    }))
}

/// One document entry.
fn load_document(at: &str, entry: &Json) -> Result<Document, Error> {
    let Some(fields) = entry.as_object() else {
        return Err(Error::Invalid(format!(
            "{at}: every entry of `documents` must be a mapping"
        )));
    };
    reject_unknown(at, "documents[]", entry, &DOCUMENT_KEYS)?;

    let name = fields
        .get("name")
        .and_then(Json::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Error::Invalid(format!("{at}: a document has no `name`")))?;

    Ok(Document {
        name: name.to_owned(),
        source: load_source(at, name, fields.get("source"))?,
        images: load_images(at, name, fields.get("images"))?,
        consumers: load_consumers(at, name, fields.get("consumers"))?,
        exempt: load_exemptions(at, name, fields.get("exempt"))?,
    })
}

/// Where the document is rendered.
fn load_source(at: &str, name: &str, source: Option<&Json>) -> Result<Source, Error> {
    let Some(fields) = source.and_then(Json::as_object) else {
        return Err(Error::Invalid(format!("{at}: {name}: `source` is missing")));
    };
    reject_unknown(
        at,
        &format!("{name}.source"),
        source.unwrap_or(&Json::Null),
        &SOURCE_KEYS,
    )?;
    for required in ["kind", "key"] {
        if fields.get(required).and_then(Json::as_str).is_none() {
            return Err(Error::Invalid(format!(
                "{at}: {name}: `source.{required}` is missing"
            )));
        }
    }

    let spelling = fields
        .get("format")
        .and_then(Json::as_str)
        .unwrap_or("toml");
    let format = DocumentFormat::parse(spelling).ok_or_else(|| {
        Error::Invalid(format!(
            "{at}: {name}: `source.format` {} is not one of toml, json, yaml",
            crate::gate::quoted(spelling)
        ))
    })?;

    Ok(Source {
        kind: fields["kind"].as_str().unwrap_or_default().to_owned(),
        selector: selector(fields.get("selector")),
        key: fields["key"].as_str().unwrap_or_default().to_owned(),
        format,
    })
}

/// The images a document is read by.
fn load_images(at: &str, name: &str, images: Option<&Json>) -> Result<Vec<ImageRef>, Error> {
    let entries = images
        .and_then(Json::as_array)
        .filter(|list| !list.is_empty());
    let Some(entries) = entries else {
        return Err(Error::Invalid(format!(
            "{at}: {name}: `images` is missing or empty"
        )));
    };

    let mut loaded = Vec::new();
    for image in entries {
        let Some(fields) = image.as_object() else {
            return Err(Error::Invalid(format!(
                "{at}: {name}: every entry of `images` must be a mapping"
            )));
        };
        reject_unknown(at, &format!("{name}.images[]"), image, &IMAGE_KEYS)?;
        for required in ["values", "contract"] {
            if fields.get(required).and_then(Json::as_str).is_none() {
                return Err(Error::Invalid(format!(
                    "{at}: {name}: `images[].{required}` is missing"
                )));
            }
        }
        loaded.push(ImageRef {
            values: fields["values"].as_str().unwrap_or_default().to_owned(),
            contract: fields["contract"].as_str().unwrap_or_default().to_owned(),
        });
    }
    Ok(loaded)
}

/// The workloads that mount a document.
fn load_consumers(at: &str, name: &str, consumers: Option<&Json>) -> Result<Vec<Consumer>, Error> {
    let mut loaded = Vec::new();
    for consumer in consumers.and_then(Json::as_array).into_iter().flatten() {
        let Some(fields) = consumer.as_object() else {
            return Err(Error::Invalid(format!(
                "{at}: {name}: every entry of `consumers` must be a mapping"
            )));
        };
        reject_unknown(at, &format!("{name}.consumers[]"), consumer, &CONSUMER_KEYS)?;

        let workload = fields.get("workload").and_then(Json::as_object);
        let kind = workload.and_then(|workload| workload.get("kind").and_then(Json::as_str));
        let Some(kind) = kind else {
            return Err(Error::Invalid(format!(
                "{at}: {name}: `consumers[].workload.kind` is missing"
            )));
        };

        let containers = fields
            .get("containers")
            .and_then(Json::as_array)
            .filter(|list| !list.is_empty());
        let Some(containers) = containers else {
            return Err(Error::Invalid(format!(
                "{at}: {name}: `consumers[].containers` is missing or empty"
            )));
        };

        loaded.push(Consumer {
            kind: kind.to_owned(),
            selector: selector(workload.and_then(|workload| workload.get("selector"))),
            containers: containers.iter().map(text_of).collect(),
        });
    }
    Ok(loaded)
}

/// The per-values-file exemptions.
fn load_exemptions(at: &str, name: &str, exempt: Option<&Json>) -> Result<Vec<Exemption>, Error> {
    let mut loaded = Vec::new();
    for exemption in exempt.and_then(Json::as_array).into_iter().flatten() {
        let Some(fields) = exemption.as_object() else {
            return Err(Error::Invalid(format!(
                "{at}: {name}: every entry of `exempt` must be a mapping"
            )));
        };
        reject_unknown(at, &format!("{name}.exempt[]"), exemption, &EXEMPT_KEYS)?;

        let gates = fields
            .get("gates")
            .and_then(Json::as_array)
            .filter(|list| !list.is_empty());
        let Some(gates) = gates else {
            return Err(Error::Invalid(format!(
                "{at}: {name}: an `exempt` entry names no `gates`"
            )));
        };
        let named: Vec<String> = gates.iter().map(text_of).collect();
        let known: Vec<&str> = Relaxed::GATES.iter().map(|(gate, _)| *gate).collect();
        let unknown: Vec<&str> = {
            let mut found: Vec<&str> = named
                .iter()
                .map(String::as_str)
                .filter(|gate| !known.contains(gate))
                .collect();
            found.sort_unstable();
            found.dedup();
            found
        };
        if !unknown.is_empty() {
            return Err(Error::Invalid(format!(
                "{at}: {name}: `exempt` names gates that do not exist: {} (known: {})",
                unknown.join(", "),
                known.join(", ")
            )));
        }

        let reason = fields
            .get("reason")
            .and_then(Json::as_str)
            .filter(|reason| !reason.is_empty());
        let Some(reason) = reason else {
            return Err(Error::Invalid(format!(
                "{at}: {name}: the exemption for {} has no `reason`; an unexplained hole in a gate \
                 is indistinguishable from an oversight",
                shown(fields.get("values"))
            )));
        };

        loaded.push(Exemption {
            values: fields
                .get("values")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .to_owned(),
            gates: named,
            reason: reason.to_owned(),
        });
    }
    Ok(loaded)
}

/// The keys this chart binds no value to.
fn load_unbound(
    at: &str,
    entries: Option<&Json>,
    documents: &BTreeSet<&str>,
) -> Result<Vec<Unbound>, Error> {
    let mut loaded: Vec<Unbound> = Vec::new();
    let mut seen: BTreeMap<(String, Option<String>), usize> = BTreeMap::new();

    for (position, entry) in entries
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let where_ = format!("unbound[{position}]");
        let Some(fields) = entry.as_object() else {
            return Err(Error::Invalid(format!(
                "{at}: every entry of `unbound` must be a mapping"
            )));
        };
        reject_unknown(at, &where_, entry, &UNBOUND_KEYS)?;

        let keys = fields
            .get("keys")
            .and_then(Json::as_array)
            .filter(|list| !list.is_empty());
        let Some(keys) = keys else {
            return Err(Error::Invalid(format!(
                "{at}: {where_}: `keys` is missing or empty; an entry writes off at least one key, \
                 listed by name — there is no pattern form, so a key an image release adds cannot \
                 be written off by something somebody typed before it existed"
            )));
        };
        let named: Vec<String> = keys
            .iter()
            .map(|key| {
                key.as_str()
                    .filter(|key| !key.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        Error::Invalid(format!(
                            "{at}: {where_}: every entry of `keys` must be a name"
                        ))
                    })
            })
            .collect::<Result<_, _>>()?;

        let reason = fields
            .get("reason")
            .and_then(Json::as_str)
            .filter(|reason| !reason.is_empty());
        let Some(reason) = reason else {
            return Err(Error::Invalid(format!(
                "{at}: {where_}: no `reason`; a key nothing surfaces is either a decision or the \
                 oversight this gate exists to catch, and silence cannot say which"
            )));
        };

        let scope = unbound_scope(at, &where_, fields.get("documents"), documents)?;

        for key in &named {
            let scopes: Vec<Option<String>> = match &scope {
                Some(listed) => listed.iter().map(|held| Some(held.clone())).collect(),
                None => vec![None],
            };
            for name in scopes {
                if let Some(first) = seen.get(&(key.clone(), name.clone())) {
                    return Err(Error::Invalid(format!(
                        "{at}: {where_}: {} is already written off by unbound[{first}]; two \
                         reasons for one key means one of them is not the reason",
                        crate::gate::quoted(key)
                    )));
                }
                seen.insert((key.clone(), name), position);
            }
        }

        loaded.push(Unbound {
            keys: named,
            reason: reason.to_owned(),
            documents: scope,
        });
    }
    Ok(loaded)
}

/// The documents one `unbound` entry is scoped to, when it names any.
///
/// Absent means every document that declares the key, which is what the fact usually is: a key
/// nothing surfaces is unsurfaced everywhere it is declared. Naming documents says the key is
/// surfaced in some of them and not others, and naming one the chart does not declare is a scope
/// that silently covers nothing.
fn unbound_scope(
    at: &str,
    where_: &str,
    scope: Option<&Json>,
    documents: &BTreeSet<&str>,
) -> Result<Option<Vec<String>>, Error> {
    let Some(scope) = scope.filter(|held| !held.is_null()) else {
        return Ok(None);
    };
    let Some(listed) = scope.as_array().filter(|list| !list.is_empty()) else {
        return Err(Error::Invalid(format!(
            "{at}: {where_}: `documents` scopes the write-off and must be a non-empty list; leave              it out to write the keys off wherever they are declared"
        )));
    };

    let named: Vec<String> = listed.iter().map(text_of).collect();
    let unknown: Vec<String> = {
        let mut found: Vec<String> = named
            .iter()
            .filter(|name| !documents.contains(name.as_str()))
            .map(|name| crate::gate::quoted(name))
            .collect();
        found.sort();
        found.dedup();
        found
    };
    if !unknown.is_empty() {
        return Err(Error::Invalid(format!(
            "{at}: {where_}: `documents` names {}, which this chart does not declare (declared:              {})",
            unknown.join(", "),
            documents.iter().copied().collect::<Vec<_>>().join(", ")
        )));
    }
    Ok(Some(named))
}

/// What the chart adds to each credential's row, keyed by contract path.
fn load_credentials(
    at: &str,
    entries: Option<&Json>,
) -> Result<BTreeMap<String, CredentialNote>, Error> {
    let mut notes: BTreeMap<String, CredentialNote> = BTreeMap::new();
    for entry in entries.and_then(Json::as_array).into_iter().flatten() {
        let Some(fields) = entry.as_object() else {
            return Err(Error::Invalid(format!(
                "{at}: every entry of `credentials` must be a mapping carrying `key` and at least \
                 one of `value` and `note`"
            )));
        };
        reject_unknown(at, "credentials[]", entry, &CREDENTIAL_KEYS)?;

        let key = fields
            .get("key")
            .and_then(Json::as_str)
            .filter(|key| !key.is_empty());
        let Some(key) = key else {
            return Err(Error::Invalid(format!(
                "{at}: an entry of `credentials` names no `key`"
            )));
        };
        if notes.contains_key(key) {
            return Err(Error::Invalid(format!(
                "{at}: {} is named by two `credentials` entries, so one of the two is stale",
                crate::gate::quoted(key)
            )));
        }

        let value = optional_text(at, fields.get("value"), key, "value")?;
        let note = optional_text(at, fields.get("note"), key, "note")?;
        if value.is_none() && note.is_none() {
            return Err(Error::Invalid(format!(
                "{at}: the `credentials` entry for {} adds neither a `value` nor a `note`, and the \
                 row it describes is generated from the contract without it",
                crate::gate::quoted(key)
            )));
        }

        notes.insert(
            key.to_owned(),
            CredentialNote {
                key: key.to_owned(),
                value: value.map(|held| held.trim().to_owned()),
                // Collapsed to single spaces: the note is a table cell, and a YAML folded block
                // arrives carrying the line breaks its author wrapped it at.
                note: note.map(|held| held.split_whitespace().collect::<Vec<_>>().join(" ")),
            },
        );
    }
    Ok(notes)
}

/// One optional non-empty string field of a credential entry.
fn optional_text(
    at: &str,
    value: Option<&Json>,
    key: &str,
    field: &str,
) -> Result<Option<String>, Error> {
    match value {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(held)) if !held.trim().is_empty() => Ok(Some(held.clone())),
        _ => Err(Error::Invalid(format!(
            "{at}: the `credentials` entry for {} carries an empty `{field}`",
            crate::gate::quoted(key)
        ))),
    }
}

/// Refuse a key nobody reads, which for a file whose job is to be exhaustive is a typo.
fn reject_unknown(at: &str, where_: &str, mapping: &Json, allowed: &[&str]) -> Result<(), Error> {
    let mut unknown: Vec<&str> = mapping
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| name.as_str())
        .filter(|name| !allowed.contains(name))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    unknown.sort_unstable();
    let location = if where_.is_empty() {
        String::new()
    } else {
        format!("{where_}: ")
    };
    Err(Error::Invalid(format!(
        "{at}: {location}unknown key(s) {}; known keys are {}",
        unknown.join(", "),
        allowed.join(", ")
    )))
}

/// A label selector, in the order it was written.
fn selector(value: Option<&Json>) -> Vec<(String, String)> {
    value
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
        .map(|(name, held)| (name.clone(), text_of(held)))
        .collect()
}

/// Every string in a list field, and nothing when there is none.
fn strings(value: Option<&Json>) -> Vec<String> {
    value
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .map(text_of)
        .collect()
}

/// One scalar as the text a declaration meant by it.
fn text_of(value: &Json) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// One YAML file as JSON, with an absent or empty document reading as an empty mapping.
///
/// # Errors
/// [`Error::Invalid`] when the text is not YAML.
pub fn read_yaml(text: &str, path: &Path) -> Result<Json, Error> {
    if text.trim().is_empty() {
        return Ok(Json::Object(serde_json::Map::new()));
    }
    let mut documents = serde_norway::Deserializer::from_str(text);
    let Some(first) = documents.next() else {
        return Ok(Json::Object(serde_json::Map::new()));
    };
    let value = Json::deserialize(first)
        .map_err(|e| Error::Invalid(format!("{}: is not valid YAML: {e}", path.display())))?;
    Ok(if value.is_null() {
        Json::Object(serde_json::Map::new())
    } else {
        value
    })
}

// ------------------------------------------------------------------------------------------
// Walking the tree
// ------------------------------------------------------------------------------------------

/// Every chart directory under `charts`, sorted.
///
/// "Is this a chart" is `Chart.yaml` and nothing else — not a name list, not an exclusion set — so a
/// chart added to the tree is picked up by every rule at once with nothing to update. Sorted, so
/// every report and every generated file is ordered by the tree rather than by whatever order the
/// filesystem happened to return.
///
/// # Errors
/// [`Error::Io`] when the directory cannot be listed.
pub fn chart_dirs(charts: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut found: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(charts).map_err(|e| Error::io(charts.display(), e))? {
        let entry = entry.map_err(|e| Error::io(charts.display(), e))?;
        if entry.path().join("Chart.yaml").is_file() {
            found.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}

/// Every chart carrying a declaration, paired with it.
///
/// `documents_only` is a real distinction rather than a convenience. A chart with `documents: []`
/// has opted out explicitly and carries a reason: an `explain` command must see it, because "this
/// chart opted out, and here is why" is an answer somebody ran the command to get. A gate must not,
/// because there is no document for it to read.
///
/// # Errors
/// [`Error::Invalid`] when any declaration cannot be read as one.
pub fn declared(charts: &Path, documents_only: bool) -> Result<Vec<(PathBuf, Declaration)>, Error> {
    let mut found = Vec::new();
    for chart_dir in chart_dirs(charts)? {
        let Some(declaration) = load_declaration(&chart_dir)? else {
            continue;
        };
        if documents_only && declaration.documents.is_empty() {
            continue;
        }
        found.push((chart_dir, declaration));
    }
    Ok(found)
}

// ------------------------------------------------------------------------------------------
// Resolving the image a chart pins
// ------------------------------------------------------------------------------------------

/// What a chart's image template would render from one values path, split into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedImage {
    /// The reference as it would be written into a manifest.
    pub reference: String,
    /// Registry and repository, with Docker Hub spelled out.
    pub normalized: String,
    /// The digest, when the tag pins one inline.
    pub digest: Option<String>,
}

/// Build the image reference a chart renders for one values path.
///
/// An empty `registry` means Docker Hub, `tag` falls back to the chart's `appVersion`, and the tag
/// may pin a digest inline — which is what actually pins the pull, and what a contract is tied to.
///
/// # Errors
/// [`Error::Invalid`] when the path does not resolve to an image block.
pub fn resolve_image(
    values: &Json,
    path: &str,
    app_version: Option<&str>,
) -> Result<PinnedImage, Error> {
    let image = dig(values, path).and_then(Json::as_object);
    let repository = image
        .and_then(|image| image.get("repository"))
        .and_then(Json::as_str)
        .filter(|repository| !repository.is_empty());
    let (Some(image), Some(repository)) = (image, repository) else {
        return Err(Error::Invalid(format!(
            "values path {} does not resolve to an image with a repository",
            crate::gate::quoted(path)
        )));
    };

    let registry = image
        .get("registry")
        .and_then(Json::as_str)
        .unwrap_or_default();
    let tag = image
        .get("tag")
        .and_then(Json::as_str)
        .filter(|tag| !tag.is_empty())
        .or(app_version)
        .unwrap_or_default();

    let mut reference = if registry.is_empty() {
        repository.to_owned()
    } else {
        format!("{registry}/{repository}")
    };
    if !tag.is_empty() {
        reference = format!("{reference}:{tag}");
    }

    Ok(PinnedImage {
        reference,
        normalized: format!(
            "{}/{repository}",
            if registry.is_empty() {
                "docker.io"
            } else {
                registry
            }
        ),
        digest: tag.split_once('@').map(|(_, digest)| digest.to_owned()),
    })
}

// ------------------------------------------------------------------------------------------
// The vendored contracts
// ------------------------------------------------------------------------------------------

/// One `charts/<chart>/contracts/<name>.json`: a published contract and its provenance.
///
/// The published document deliberately carries no image digest — a digest is what building the
/// image *produces*, so a field holding it would have to be written after the push, changing the
/// bytes it was computed over. The tie is the attachment instead: whatever comes back from asking a
/// digest for its referrers belongs to that digest. The chart repository records which digest that
/// was on the way in, and that record is what the staleness interlock reads.
#[derive(Debug, Clone)]
pub struct Vendored {
    /// Where the file is.
    pub path: PathBuf,
    /// The image it came from, registry and repository.
    pub image: String,
    /// The digest it was fetched for.
    pub digest: String,
    /// The hash of the document as published.
    pub sha256: String,
    /// When it was fetched.
    pub fetched: String,
    /// The contract itself, as published.
    pub contract: Json,
}

impl Vendored {
    /// The release this contract was published at, spelled the way `appVersion` is compared.
    ///
    /// Empty for a contract carrying no `app.version`, which is a fixture rather than anything a
    /// registry serves; the caller decides what to do with that rather than being handed a guess.
    pub fn published_version(&self) -> &str {
        self.contract
            .get("app")
            .and_then(|app| app.get("version"))
            .and_then(Json::as_str)
            .map_or("", release)
    }
}

/// Read and shape-check one vendored contract.
///
/// `source.sha256` is over the document *as published*, so it is verifiable only against the image
/// label, which needs a registry. Offline it is provenance: it records which bytes were verified, so
/// a later networked run can prove the committed copy is still the one the image carries.
///
/// # Errors
/// [`Error::Io`] when the file cannot be read, [`Error::Invalid`] when it is not shaped like one.
pub fn load_vendored(path: &Path) -> Result<Vendored, Error> {
    let at = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(&at, e))?;
    let document: Json = serde_json::from_str(&text)
        .map_err(|e| Error::Invalid(format!("{at}: is not valid JSON: {e}")))?;
    if !document.is_object() {
        return Err(Error::Invalid(format!(
            "{at}: expected an object at the top level"
        )));
    }

    let Some(source) = document.get("source").and_then(Json::as_object) else {
        return Err(Error::Invalid(format!(
            "{at}: missing the `source` provenance object"
        )));
    };
    let mut held = [""; 4];
    for (index, name) in ["image", "digest", "sha256", "fetched"]
        .into_iter()
        .enumerate()
    {
        held[index] = source
            .get(name)
            .and_then(Json::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                Error::Invalid(format!("{at}: `source.{name}` is missing or not a string"))
            })?;
    }
    if !held[1].starts_with("sha256:") {
        return Err(Error::Invalid(format!(
            "{at}: `source.digest` is not a sha256 digest: {}",
            held[1]
        )));
    }

    let Some(contract) = document.get("contract").filter(|held| held.is_object()) else {
        return Err(Error::Invalid(format!(
            "{at}: missing the `contract` document"
        )));
    };

    Ok(Vendored {
        path: path.to_path_buf(),
        image: held[0].to_owned(),
        digest: held[1].to_owned(),
        sha256: held[2].to_owned(),
        fetched: held[3].to_owned(),
        contract: contract.clone(),
    })
}

/// One vendored contract, the reference that named it, and the label it is reported under.
///
/// The label is `<chart>/<path>`, and it is built here rather than at each call site because it is
/// the string every message and every union source list identifies a contract by.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The declaration entry that named it.
    pub reference: ImageRef,
    /// What messages call it.
    pub label: String,
    /// What it holds.
    pub vendored: Vendored,
}

/// Load every contract one document binds. **Applies no staleness interlock.**
///
/// The bold part is the whole reason this exists rather than each caller writing its own three
/// lines. Whether the vendored file is for the digest the chart *currently* pins is a separate
/// question from what the file says, and callers legitimately answer it differently: a gate wants
/// the interlock, because a pass it cannot justify is worse than a failure, and a generator does
/// not, because withholding the document during a digest bump removes it at the moment it is being
/// read. Each of those is a considered decision, and each was previously expressed as *the absence*
/// of code — a four-line loop that looked like a helper and was in fact a deliberate bypass.
///
/// # Errors
/// [`Error::Invalid`] on a file that cannot be read or is not shaped like a contract. That is a
/// parse rather than an interlock, and no caller wants to proceed past it.
pub fn vendored_for(chart_dir: &Path, document: &Document) -> Result<Vec<Loaded>, Error> {
    let chart = chart_dir
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    document
        .images
        .iter()
        .map(|reference| {
            Ok(Loaded {
                label: format!("{chart}/{}", reference.contract),
                vendored: load_vendored(&chart_dir.join(&reference.contract))?,
                reference: reference.clone(),
            })
        })
        .collect()
}

/// The contracts one document is validated against, in the two scopes the gates need.
///
/// `union` is every image that reads the document, for gate 1. `by_digest` is one image each, for
/// gates 2 and 3 — held as single-contract unions so both scopes are the same type to a rule.
#[derive(Debug, Clone)]
pub struct Binding {
    /// Every image that reads the document, merged.
    pub union: Union,
    /// One image each, by the digest a container pins.
    pub by_digest: BTreeMap<String, Union>,
}

/// Load, interlock and merge every contract this document is validated against.
///
/// Returns no binding rather than a partial answer, and that is the staleness interlock. A digest
/// bump produces one run holding a new digest and the *old* contract, before whatever refreshes the
/// vendored copies commits. Validating anyway would report a pass it cannot justify, on exactly the
/// pull request the whole design exists to protect. Deterministic, offline, self-healing, and a hard
/// failure rather than a skip.
///
/// # Errors
/// [`Error::Invalid`] when a contract cannot be read or two cannot be reconciled — as distinct from
/// the problems returned beside the binding, which are about this chart's own pins.
pub fn bind(
    chart_dir: &Path,
    document: &Document,
    values: &Json,
    app_version: Option<&str>,
) -> Result<(Option<Binding>, Vec<String>), Error> {
    let loaded = match vendored_for(chart_dir, document) {
        Ok(loaded) => loaded,
        Err(failure) => return Ok((None, vec![failure.to_string()])),
    };

    let mut problems: Vec<String> = Vec::new();
    let mut contracts: Vec<(String, Json)> = Vec::new();
    let mut by_digest: BTreeMap<String, Union> = BTreeMap::new();

    for item in &loaded {
        let pinned = match resolve_image(values, &item.reference.values, app_version) {
            Ok(pinned) => pinned,
            Err(failure) => return Ok((None, vec![failure.to_string()])),
        };

        let Some(digest) = pinned.digest.clone() else {
            problems.push(format!(
                "the values path {} resolves to {}, which is not pinned by digest; a contract \
                 cannot be tied to a mutable tag",
                crate::gate::quoted(&item.reference.values),
                pinned.reference
            ));
            continue;
        };

        if item.vendored.digest != digest {
            problems.push(format!(
                // Leads with the instruction that always works. Advice that assumes a refresh job
                // is coming reads, on exactly the runs where it is not, as "wait" — and waiting is
                // what produced a pull request re-run three times against a job that failed
                // identically each time.
                "{} is for {}, but the chart pins {digest}. Refresh the vendored contracts and \
                 commit the result.",
                item.label, item.vendored.digest
            ));
            continue;
        }

        if item.vendored.image != pinned.normalized {
            problems.push(format!(
                "{} is for the image {}, but the values path {} resolves to {}",
                item.label,
                item.vendored.image,
                crate::gate::quoted(&item.reference.values),
                pinned.normalized
            ));
            continue;
        }

        contracts.push((item.label.clone(), item.vendored.contract.clone()));
        by_digest.insert(
            digest,
            union_contracts(&[(item.label.clone(), item.vendored.contract.clone())])?,
        );
    }

    if !problems.is_empty() {
        return Ok((None, problems));
    }

    let union = union_contracts(&contracts)?;
    let offenders = local_refs_only(&union.json_schema);
    if !offenders.is_empty() {
        return Ok((
            None,
            offenders
                .into_iter()
                .map(|offender| {
                    format!(
                        "the merged schema carries a remote reference, which would make this \
                         offline gate a networked one: {offender}"
                    )
                })
                .collect(),
        ));
    }

    Ok((Some(Binding { union, by_digest }), Vec::new()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::{load_declaration, read_yaml, resolve_image};

    fn write(body: &str) -> tempdir::Dir {
        let dir = tempdir::Dir::new();
        std::fs::write(dir.path().join("Chart.yaml"), "name: x\n").expect("the chart is written");
        std::fs::write(dir.path().join("config-contract.yaml"), body)
            .expect("the declaration is written");
        dir
    }

    /// A directory that removes itself, so a test that reads a real file needs no fixture tree.
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU32, Ordering};

        static NEXT: AtomicU32 = AtomicU32::new(0);

        pub(super) struct Dir(PathBuf);

        impl Dir {
            pub(super) fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "terrace-contract-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::create_dir_all(&path).expect("a temporary directory is created");
                Self(path)
            }
            pub(super) fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn a_key_nobody_reads_is_a_typo_rather_than_something_to_ignore() {
        let dir = write("documents: []\nreason: not adopted\nunbounded: []\n");
        let error = load_declaration(dir.path()).expect_err("an unknown key is refused");
        assert!(
            error.to_string().contains("unknown key(s) unbounded"),
            "{error}"
        );
    }

    #[test]
    fn an_empty_document_list_is_an_opt_out_and_needs_a_reason() {
        let dir = write("documents: []\n");
        let error = load_declaration(dir.path()).expect_err("an unexplained opt-out is refused");
        assert!(error.to_string().contains("explicit opt-out"), "{error}");

        let dir = write("documents: []\nreason: the image publishes none yet\n");
        let declaration = load_declaration(dir.path())
            .expect("an explained opt-out reads")
            .expect("the chart has a declaration");
        assert!(declaration.documents.is_empty());
        assert_eq!(
            declaration.reason.as_deref(),
            Some("the image publishes none yet")
        );
    }

    #[test]
    fn a_chart_with_no_declaration_is_skipped_rather_than_failed() {
        let dir = tempdir::Dir::new();
        assert!(
            load_declaration(dir.path())
                .expect("no declaration is not an error")
                .is_none()
        );
    }

    #[test]
    fn an_exemption_names_a_gate_that_exists_and_says_why() {
        let dir = write(
            "documents:\n  - name: app\n    source: {kind: ConfigMap, key: config.toml}\n    \
             images: [{values: image, contract: contracts/app.json}]\n    exempt:\n      \
             - {values: 'ci/a.yaml', gates: [gate2], reason: x}\n",
        );
        let error =
            load_declaration(dir.path()).expect_err("a gate that does not exist is refused");
        assert!(
            error.to_string().contains("gates that do not exist: gate2"),
            "{error}"
        );

        let dir = write(
            "documents:\n  - name: app\n    source: {kind: ConfigMap, key: config.toml}\n    \
             images: [{values: image, contract: contracts/app.json}]\n    exempt:\n      \
             - {values: 'ci/a.yaml', gates: [env]}\n",
        );
        let error = load_declaration(dir.path()).expect_err("an unexplained exemption is refused");
        assert!(
            error
                .to_string()
                .contains("indistinguishable from an oversight"),
            "{error}"
        );
    }

    #[test]
    fn an_exemption_applies_to_the_values_file_it_names_and_a_star_to_every_one() {
        let dir = write(
            "documents:\n  - name: app\n    source: {kind: ConfigMap, key: config.toml}\n    \
             images: [{values: image, contract: contracts/app.json}]\n    exempt:\n      \
             - {values: 'ci/a.yaml', gates: [env], reason: r}\n      \
             - {values: '*', gates: [closed], reason: r}\n",
        );
        let declaration = load_declaration(dir.path())
            .expect("the declaration reads")
            .expect("the chart has one");
        let document = &declaration.documents[0];
        assert!(document.relaxed("a.yaml").env);
        assert!(!document.relaxed("b.yaml").env);
        assert!(document.relaxed("b.yaml").closed);
    }

    #[test]
    fn two_reasons_for_one_unbound_key_means_one_of_them_is_not_the_reason() {
        let dir = write(
            "documents: []\nreason: r\nunbound:\n  - {keys: [a], reason: one}\n  \
             - {keys: [a], reason: two}\n",
        );
        let error = load_declaration(dir.path()).expect_err("a key written off twice is refused");
        assert!(error.to_string().contains("already written off"), "{error}");
    }

    #[test]
    fn an_unbound_scope_must_name_a_document_the_chart_declares() {
        let dir = write(
            "documents:\n  - name: app\n    source: {kind: ConfigMap, key: config.toml}\n    \
             images: [{values: image, contract: contracts/app.json}]\nunbound:\n  \
             - {keys: [a], reason: r, documents: [other]}\n",
        );
        let error = load_declaration(dir.path()).expect_err("an unknown document is refused");
        assert!(error.to_string().contains("does not declare"), "{error}");
    }

    #[test]
    fn a_credential_row_adds_something_or_is_not_written() {
        let dir = write("documents: []\nreason: r\ncredentials:\n  - {key: a}\n");
        let error = load_declaration(dir.path()).expect_err("an empty row is refused");
        assert!(error.to_string().contains("adds neither"), "{error}");
    }

    #[test]
    fn an_image_reference_falls_back_to_the_charts_app_version() {
        let values = json!({"image": {"repository": "ghcr.io/x/y"}});
        let pinned =
            resolve_image(&values, "image", Some("v1.2.3@sha256:ab")).expect("the path resolves");
        assert_eq!(pinned.reference, "ghcr.io/x/y:v1.2.3@sha256:ab");
        assert_eq!(pinned.normalized, "docker.io/ghcr.io/x/y");
        assert_eq!(pinned.digest.as_deref(), Some("sha256:ab"));
    }

    #[test]
    fn a_values_path_that_pins_nothing_is_refused_by_name() {
        let error = resolve_image(&json!({}), "image", None).expect_err("nothing resolves");
        assert!(error.to_string().contains("'image'"), "{error}");
    }

    #[test]
    fn an_empty_file_reads_as_an_empty_mapping() {
        let empty = read_yaml("", Path::new("x")).expect("an empty file reads");
        assert_eq!(empty, json!({}));
        assert_eq!(
            read_yaml("---\n", Path::new("x")).expect("a null document reads"),
            json!({})
        );
    }
}
