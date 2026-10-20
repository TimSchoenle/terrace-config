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
//!
//! # A key left out means its default, and the default is part of the layer version
//!
//! Most of a single-image chart's declaration is the same in every chart: the document is a
//! `ConfigMap` labelled with the release, under `config.toml`, read by the image pinned at `image`,
//! whose contract is vendored under the document's name, and mounted by whichever workloads mount
//! it. Each of those may be left out, and [`Document::defaulted`] records which were, so
//! `config show --chart` can say what a gate is actually holding the chart to. Every default is
//! listed in `spec/helm/v1/DEFAULTS.md`; changing one is a breaking change, because a chart relying
//! on it has written nothing that would reveal the change. A key that is written always wins.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize as _;
use serde_json::Value as Json;

use crate::error::Error;
use crate::gate::{DocumentFormat, DocumentSource, Relaxed};
use crate::union::{Ordered, Union, local_refs_only, union_contracts};

use super::{DECLARATION, dig, release, shown};

/// Keys the top level may carry.
const DECLARATION_KEYS: [&str; 7] = [
    "bindings",
    "credentials",
    "documents",
    "reason",
    "tests",
    "unbound",
    "unconfigured",
];
const CREDENTIAL_KEYS: [&str; 3] = ["key", "note", "value"];
const DOCUMENT_KEYS: [&str; 7] = [
    "consumers",
    "exempt",
    "images",
    "name",
    "restart",
    "source",
    "tests",
];
const RESTART_KEYS: [&str; 2] = ["keys", "reason"];
const SOURCE_KEYS: [&str; 4] = ["format", "key", "kind", "selector"];
const IMAGE_KEYS: [&str; 2] = ["contract", "values"];
const CONSUMER_KEYS: [&str; 2] = ["containers", "workload"];
const EXEMPT_KEYS: [&str; 3] = ["gates", "reason", "values"];
const UNBOUND_KEYS: [&str; 3] = ["documents", "keys", "reason"];

/// Where an editor finds the schema a declaration is written against.
///
/// An identifier as much as an address: the CLI reads the declaration itself and never fetches this.
pub const DECLARATION_SCHEMA_URL: &str = "https://raw.githubusercontent.com/TimSchoenle/terrace-config/main/spec/helm/v1/declaration.schema.json";

/// The kind of object a document is rendered into when `source.kind` is left out.
pub const DEFAULT_SOURCE_KIND: &str = "ConfigMap";

/// The key a document is rendered under when `source.key` is left out.
pub const DEFAULT_SOURCE_KEY: &str = "config.toml";

/// The label a default selector matches, valued with the release name.
///
/// `.Release.Name` is the one identifier neither `nameOverride` nor `fullnameOverride` can move, so
/// it is the one label a selector can rely on under every values file.
pub const RELEASE_LABEL: &str = "app.kubernetes.io/instance";

/// The values path of the image a document is read by when `images` is left out.
pub const DEFAULT_IMAGE_VALUES: &str = "image";

/// The directory, relative to the chart, a default contract path is written under.
pub const CONTRACTS_DIR: &str = "contracts";

/// The object kinds whose readers can be found from the rendered manifests.
///
/// Both are mounted as volumes by name, which is what inference follows. Any other kind has no
/// reference a workload's pod spec spells, so its consumers have to be written out.
const INFERABLE_KINDS: [&str; 2] = ["ConfigMap", "Secret"];

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
    /// The workload's rendered name, for a consumer found in one rendered file rather than written
    /// into the declaration. A declared consumer never carries one: a name override moves it.
    pub name: Option<String>,
    /// The containers inside it that read the document.
    pub containers: Vec<String>,
}

impl Consumer {
    /// The rendered workloads this consumer names.
    #[must_use]
    pub fn select<'a>(&self, manifests: &'a [Json]) -> Vec<&'a Json> {
        let mut matched = crate::k8s::select(manifests, &self.kind, &self.selector);
        if let Some(name) = &self.name {
            matched.retain(|workload| {
                workload
                    .get("metadata")
                    .and_then(|metadata| metadata.get("name"))
                    .and_then(Json::as_str)
                    == Some(name.as_str())
            });
        }
        matched
    }
}

/// The workloads that read a document: written out, or found in each rendered file.
#[derive(Debug, Clone)]
pub enum Consumers {
    /// Listed in the declaration. An empty list is a written decision to check no workload.
    Declared(Vec<Consumer>),
    /// Left out: every workload whose pod spec mounts the rendered source object, and the
    /// containers of it that mount it, found again in every rendered values file.
    Inferred,
}

impl Consumers {
    /// The consumers written in the declaration; none when they are inferred.
    #[must_use]
    pub fn declared(&self) -> &[Consumer] {
        match self {
            Self::Declared(listed) => listed,
            Self::Inferred => &[],
        }
    }

    /// Whether the consumers are found from the rendered manifests.
    #[must_use]
    pub const fn inferred(&self) -> bool {
        matches!(self, Self::Inferred)
    }
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

/// Contract keys a chart rolls its pods for although an image applies them without a restart.
///
/// The one direction a chart may override an image in. Promoting a key to `restart` is always
/// safe — the worst it costs is a rollout nobody needed — and it is what a chart wants for a key
/// whose change should be canaried through readiness gating and a rollback rather than land on
/// every replica in one kubelet sync. The inverse does not exist: a chart cannot know better than
/// the image what the image re-reads, and a key demoted to `live` that the process only reads at
/// start is a change reported as applied and never made.
#[derive(Debug, Clone)]
pub struct Restarted {
    /// The keys, written out one by one. There is no pattern form, for [`Unbound::keys`]' reason.
    pub keys: Vec<String>,
    /// Why. Mandatory: a rollout nobody can explain is one somebody removes.
    pub reason: String,
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
    pub consumers: Consumers,
    /// The exemptions, per values file.
    pub exempt: Vec<Exemption>,
    /// Keys this chart rolls its pods for although an image applies them live.
    pub restart: Vec<Restarted>,
    /// How this document's round-trip suite is generated, when the chart is enrolled in them.
    pub tests: Option<super::suites::Enrolment>,
    /// The fields this document took from their defaults, as dotted paths under the document, in
    /// the order the declaration's reference lists them.
    pub defaulted: Vec<&'static str>,
}

impl Document {
    /// The workloads that read this document in one rendered values file.
    ///
    /// The declared ones as written, or, when the declaration leaves them out, every workload that
    /// mounts the object the source selects in this file. Inferred per file rather than once,
    /// because a values file may switch a reader on or off, and the consumers of that file are the
    /// ones it renders.
    #[must_use]
    pub fn consumers_in(&self, manifests: &[Json]) -> std::borrow::Cow<'_, [Consumer]> {
        match &self.consumers {
            Consumers::Declared(listed) => std::borrow::Cow::Borrowed(listed),
            Consumers::Inferred => {
                std::borrow::Cow::Owned(infer_consumers(manifests, &self.source))
            }
        }
    }

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

/// Every workload mounting an object the source selects, as the consumer it would be declared as.
///
/// Pinned by the rendered name as well as the kind: within one rendered file the name is fixed,
/// and labels alone could also match a workload that mounts nothing.
fn infer_consumers(manifests: &[Json], source: &Source) -> Vec<Consumer> {
    let mut found = Vec::new();
    for object in crate::k8s::select(manifests, &source.kind, &source.selector) {
        let Some(name) = object
            .get("metadata")
            .and_then(|metadata| metadata.get("name"))
            .and_then(Json::as_str)
        else {
            continue;
        };
        for mounting in crate::k8s::mounting(manifests, &source.kind, name) {
            let workload = mounting.workload;
            found.push(Consumer {
                kind: workload
                    .get("kind")
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                selector: Vec::new(),
                name: workload
                    .get("metadata")
                    .and_then(|metadata| metadata.get("name"))
                    .and_then(Json::as_str)
                    .map(str::to_owned),
                containers: mounting
                    .containers
                    .iter()
                    .map(|held| (*held).to_owned())
                    .collect(),
            });
        }
    }
    found
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
    /// Whether the chart is enrolled in the generated round-trip suites.
    ///
    /// Declared for [`Declaration::bindings`]' reason. Every document of an enrolled chart gets a
    /// suite; a document's own `tests` block only says how.
    pub tests: bool,
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
    parse_declaration(&text, chart_dir).map(Some)
}

/// Read one chart's declaration from its text, as if it were the chart's own file.
///
/// The half of [`load_declaration`] that does not touch a disk, for a caller holding a declaration
/// it has not written yet: a scaffold that hands a contributor a file no reader accepts has failed
/// on their first run rather than on its own, so it reads what it is about to write first.
///
/// # Errors
/// As [`load_declaration`].
pub fn parse_declaration(text: &str, chart_dir: &Path) -> Result<Declaration, Error> {
    let path = chart_dir.join(DECLARATION);
    let document: Json = read_yaml(text, &path)?;
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

    let bindings = enrolment_switch(&at, fields.get("bindings"), "bindings", "the marker check")?;
    let tests = enrolment_switch(&at, fields.get("tests"), "tests", "the round-trip suites")?;

    let chart = chart_dir
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let mut documents = Vec::new();
    for entry in raw {
        let document = load_document(&at, &chart, entry, tests)?;
        if documents
            .iter()
            .any(|held: &Document| held.name == document.name)
        {
            return Err(Error::Invalid(format!(
                "{at}: the document {} is declared twice",
                crate::gate::quoted(&document.name)
            )));
        }
        documents.push(document);
    }
    let names: BTreeSet<&str> = documents.iter().map(|entry| entry.name.as_str()).collect();

    Ok(Declaration {
        chart,
        path: path.clone(),
        reason,
        unconfigured: strings(fields.get("unconfigured")),
        bindings,
        unbound: load_unbound(&at, fields.get("unbound"), &names)?,
        credentials: load_credentials(&at, fields.get("credentials"))?,
        tests,
        documents,
    })
}

/// One of the declaration's enrolment switches: absent is off, and anything but a boolean is wrong.
fn enrolment_switch(at: &str, value: Option<&Json>, key: &str, what: &str) -> Result<bool, Error> {
    match value {
        None | Some(Json::Null) => Ok(false),
        Some(Json::Bool(held)) => Ok(*held),
        other => Err(Error::Invalid(format!(
            "{at}: `{key}` is the chart's enrolment in {what} and must be true or false, not {}",
            shown(other)
        ))),
    }
}

/// One document entry, with every field it leaves out taken from its default.
fn load_document(at: &str, chart: &str, entry: &Json, enrolled: bool) -> Result<Document, Error> {
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

    let mut defaulted = Vec::new();
    let source = load_source(at, name, chart, fields.get("source"), &mut defaulted)?;
    let images = load_images(at, name, fields.get("images"), &mut defaulted)?;
    let consumers = load_consumers(at, name, &source, fields.get("consumers"), &mut defaulted)?;
    let tests = load_tests(at, name, fields.get("tests"), enrolled)?;

    Ok(Document {
        name: name.to_owned(),
        source,
        images,
        consumers,
        exempt: load_exemptions(at, name, fields.get("exempt"))?,
        restart: load_restarted(at, name, fields.get("restart"))?,
        tests,
        defaulted,
    })
}

/// How one document's suite is generated: its own block, or the defaults when the chart is enrolled.
///
/// A block on a chart that is not enrolled is refused rather than read as an enrolment: the switch
/// is declared at the top for the reason `bindings` is, and a block that configures a suite nothing
/// generates is a file describing a gate that does not run.
fn load_tests(
    at: &str,
    name: &str,
    block: Option<&Json>,
    enrolled: bool,
) -> Result<Option<super::suites::Enrolment>, Error> {
    let block = block.filter(|held| !held.is_null());
    match (enrolled, block) {
        (false, None) => Ok(None),
        (false, Some(_)) => Err(Error::Invalid(format!(
            "{at}: {name}: configures a round-trip suite with `tests`, but the chart is not \
             enrolled in them; set `tests: true` at the top of the file"
        ))),
        (true, None) => Ok(Some(super::suites::Enrolment::default())),
        (true, Some(block)) => {
            if !block.is_object() {
                return Err(Error::Invalid(format!(
                    "{at}: {name}: `tests` must be a mapping; enrolment is the top-level `tests: \
                     true`"
                )));
            }
            super::suites::parse_entry(at, name, &format!("documents[{name}].tests"), block)
                .map(Some)
        }
    }
}

/// The keys a document's chart rolls its pods for although an image applies them live.
///
/// Whether each key exists is not decided here: that needs the contracts, and it is the reload
/// generator that binds them and refuses a key none of them declares.
fn load_restarted(at: &str, name: &str, entries: Option<&Json>) -> Result<Vec<Restarted>, Error> {
    let mut loaded = Vec::new();
    for (position, entry) in entries
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let where_ = format!("{name}.restart[{position}]");
        if !entry.is_object() {
            return Err(Error::Invalid(format!(
                "{at}: {name}: every entry of `restart` must be a mapping"
            )));
        }
        reject_unknown(at, &where_, entry, &RESTART_KEYS)?;

        let keys = entry
            .get("keys")
            .and_then(Json::as_array)
            .filter(|list| !list.is_empty())
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "{at}: {where_}: `keys` is missing or empty; an entry names at least one key"
                ))
            })?;
        let named: Vec<String> = keys
            .iter()
            .map(|key| {
                key.as_str()
                    .filter(|key| !key.is_empty() && !key.contains('*'))
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        Error::Invalid(format!(
                            "{at}: {where_}: every entry of `keys` must be one key's path; there is \
                             no pattern form, so a key an image adds later is never covered by \
                             something written before it existed"
                        ))
                    })
            })
            .collect::<Result<_, _>>()?;

        let reason = entry
            .get("reason")
            .and_then(Json::as_str)
            .filter(|reason| !reason.is_empty())
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "{at}: {where_}: no `reason`; a rollout nobody can explain is one somebody \
                     removes"
                ))
            })?;

        loaded.push(Restarted {
            keys: named,
            reason: reason.to_owned(),
        });
    }
    Ok(loaded)
}

/// Where the document is rendered, each field it leaves out taken from its default.
///
/// The format follows the key's extension before it falls back to TOML, so a chart rendering
/// `config.yaml` writes one key rather than two that have to agree.
fn load_source(
    at: &str,
    name: &str,
    chart: &str,
    source: Option<&Json>,
    defaulted: &mut Vec<&'static str>,
) -> Result<Source, Error> {
    let empty = serde_json::Map::new();
    let fields = match source {
        None | Some(Json::Null) => &empty,
        Some(Json::Object(fields)) => fields,
        Some(other) => {
            return Err(Error::Invalid(format!(
                "{at}: {name}: `source` must be a mapping, not {}",
                shown(Some(other))
            )));
        }
    };
    reject_unknown(
        at,
        &format!("{name}.source"),
        source.unwrap_or(&Json::Null),
        &SOURCE_KEYS,
    )?;

    let kind = text_field(at, name, fields, "kind")?.unwrap_or_else(|| {
        defaulted.push("source.kind");
        DEFAULT_SOURCE_KIND.to_owned()
    });

    let selector = match fields.get("selector") {
        None | Some(Json::Null) => {
            defaulted.push("source.selector");
            vec![(RELEASE_LABEL.to_owned(), chart.to_owned())]
        }
        Some(Json::Object(labels)) if !labels.is_empty() => selector(fields.get("selector")),
        Some(other) => {
            return Err(Error::Invalid(format!(
                "{at}: {name}: `source.selector` must be a non-empty mapping of labels, not {}; \
                 leave it out to match `{RELEASE_LABEL}: {chart}`",
                shown(Some(other))
            )));
        }
    };

    let key = text_field(at, name, fields, "key")?.unwrap_or_else(|| {
        defaulted.push("source.key");
        DEFAULT_SOURCE_KEY.to_owned()
    });

    let format = if let Some(spelling) = text_field(at, name, fields, "format")? {
        DocumentFormat::parse(&spelling).ok_or_else(|| {
            Error::Invalid(format!(
                "{at}: {name}: `source.format` {} is not one of toml, json, yaml",
                crate::gate::quoted(&spelling)
            ))
        })?
    } else {
        defaulted.push("source.format");
        format_of(&key)
    };

    Ok(Source {
        kind,
        selector,
        key,
        format,
    })
}

/// The format a key's extension names, and TOML for any other key.
fn format_of(key: &str) -> DocumentFormat {
    let extension = Path::new(key)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("json") => DocumentFormat::Json,
        Some("yaml" | "yml") => DocumentFormat::Yaml,
        _ => DocumentFormat::Toml,
    }
}

/// One optional, non-empty string field of `source`.
fn text_field(
    at: &str,
    name: &str,
    fields: &serde_json::Map<String, Json>,
    field: &str,
) -> Result<Option<String>, Error> {
    match fields.get(field) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(held)) if !held.is_empty() => Ok(Some(held.clone())),
        Some(other) => Err(Error::Invalid(format!(
            "{at}: {name}: `source.{field}` must be a non-empty string, not {}; leave it out for \
             the default",
            shown(Some(other))
        ))),
    }
}

/// The contract path a document's only image takes when its entry leaves `contract` out.
fn default_contract(document: &str) -> String {
    format!("{CONTRACTS_DIR}/{document}.json")
}

/// The images a document is read by.
///
/// Left out, the document is read by the one image pinned at [`DEFAULT_IMAGE_VALUES`]. An entry may
/// leave its `contract` out only when it is the document's one image: the default path is spelled
/// from the document's name, and two images cannot both be vendored at it.
fn load_images(
    at: &str,
    name: &str,
    images: Option<&Json>,
    defaulted: &mut Vec<&'static str>,
) -> Result<Vec<ImageRef>, Error> {
    let entries = match images {
        None | Some(Json::Null) => {
            defaulted.push("images");
            return Ok(vec![ImageRef {
                values: DEFAULT_IMAGE_VALUES.to_owned(),
                contract: default_contract(name),
            }]);
        }
        Some(Json::Array(list)) if !list.is_empty() => list,
        Some(_) => {
            return Err(Error::Invalid(format!(
                "{at}: {name}: `images` must be a non-empty list; leave it out for the one image \
                 at `{DEFAULT_IMAGE_VALUES}`"
            )));
        }
    };

    let mut loaded = Vec::new();
    for image in entries {
        let Some(fields) = image.as_object() else {
            return Err(Error::Invalid(format!(
                "{at}: {name}: every entry of `images` must be a mapping"
            )));
        };
        reject_unknown(at, &format!("{name}.images[]"), image, &IMAGE_KEYS)?;
        let Some(values) = fields.get("values").and_then(Json::as_str) else {
            return Err(Error::Invalid(format!(
                "{at}: {name}: `images[].values` is missing"
            )));
        };
        let contract = match fields.get("contract").and_then(Json::as_str) {
            Some(contract) => contract.to_owned(),
            None if entries.len() == 1 => {
                defaulted.push("images[].contract");
                default_contract(name)
            }
            None => {
                return Err(Error::Invalid(format!(
                    "{at}: {name}: `images[].contract` is missing for `{values}`; it may be left \
                     out only when the document has one image, since the default path is spelled \
                     from the document's name"
                )));
            }
        };
        loaded.push(ImageRef {
            values: values.to_owned(),
            contract,
        });
    }
    Ok(loaded)
}

/// The workloads that mount a document: the declared ones, or [`Consumers::Inferred`].
fn load_consumers(
    at: &str,
    name: &str,
    source: &Source,
    consumers: Option<&Json>,
    defaulted: &mut Vec<&'static str>,
) -> Result<Consumers, Error> {
    let entries = match consumers {
        None | Some(Json::Null) => {
            if !INFERABLE_KINDS.contains(&source.kind.as_str()) {
                return Err(Error::Invalid(format!(
                    "{at}: {name}: `consumers` is missing, and the readers of a {} cannot be \
                     found from the manifests: only a {} is mounted by name. Write them out",
                    source.kind,
                    INFERABLE_KINDS.join(" or ")
                )));
            }
            defaulted.push("consumers");
            return Ok(Consumers::Inferred);
        }
        Some(Json::Array(list)) => list,
        Some(other) => {
            return Err(Error::Invalid(format!(
                "{at}: {name}: `consumers` must be a list, not {}",
                shown(Some(other))
            )));
        }
    };

    let mut loaded = Vec::new();
    for consumer in entries {
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
            name: None,
            containers: containers.iter().map(text_of).collect(),
        });
    }
    Ok(Consumers::Declared(loaded))
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
            "{at}: {where_}: `documents` scopes the write-off and must be a non-empty list; leave \
             it out to write the keys off wherever they are declared"
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
            "{at}: {where_}: `documents` names {}, which this chart does not declare (declared: \
             {})",
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
pub(super) fn reject_unknown(
    at: &str,
    where_: &str,
    mapping: &Json,
    allowed: &[&str],
) -> Result<(), Error> {
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

/// `spec/helm/v1/declaration.schema.json`, which an editor validates a declaration against.
///
/// Embedded for the tests only. The reader above is the authority and states rules the schema
/// cannot, and running both would give one defect two messages; what the tests hold is that the two
/// agree on every key and on what each refuses.
#[cfg(test)]
pub(crate) const DECLARATION_SCHEMA: &str =
    include_str!("../../../spec/helm/v1/declaration.schema.json");

/// Every way one declaration's text breaks [`DECLARATION_SCHEMA`], as `pointer: message` lines.
#[cfg(test)]
pub(crate) fn schema_violations(text: &str) -> Vec<String> {
    let schema: Json = serde_json::from_str(DECLARATION_SCHEMA).expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let document = read_yaml(text, Path::new(DECLARATION)).expect("the declaration is YAML");
    validator
        .iter_errors(&document)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect()
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

/// Which of a tree's charts a gate reads: every one, or the ones named.
///
/// A gate over the whole tree fails a pull request for a chart it never touched, so a consumer that
/// knows what changed narrows it here. The narrowing is by directory name, which is the chart's
/// name in every tree this reads, and it is applied to the walk rather than to the findings: a chart
/// outside the selection is not read at all, so a chart that cannot even be parsed does not fail a
/// change elsewhere either.
///
/// A name matching no chart is refused rather than read as a selection of nothing. A gate that
/// checked nothing exits clean, and a typo in a pipeline would otherwise turn a gate green for good.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    names: BTreeSet<String>,
}

impl Selection {
    /// Every chart in the tree.
    #[must_use]
    pub fn all() -> Self {
        Self::default()
    }

    /// Only the named charts, or every chart when none is named.
    #[must_use]
    pub fn of<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            names: names.into_iter().map(Into::into).collect(),
        }
    }

    /// Whether this is the whole tree.
    #[must_use]
    pub fn is_all(&self) -> bool {
        self.names.is_empty()
    }

    /// Whether the chart named `name` is read.
    #[must_use]
    pub fn admits(&self, name: &str) -> bool {
        self.is_all() || self.names.contains(name)
    }

    /// The selected chart directories under `charts`, sorted as [`chart_dirs`] sorts them.
    ///
    /// # Errors
    /// [`Error::Io`] when the directory cannot be listed, and [`Error::Invalid`] naming every
    /// selected chart the tree does not hold.
    pub fn dirs(&self, charts: &Path) -> Result<Vec<PathBuf>, Error> {
        let every = chart_dirs(charts)?;
        let present: BTreeSet<&str> = every.iter().filter_map(|dir| chart_name(dir)).collect();
        let unknown: Vec<&str> = self
            .names
            .iter()
            .map(String::as_str)
            .filter(|name| !present.contains(name))
            .collect();
        if !unknown.is_empty() {
            return Err(Error::Invalid(format!(
                "{}: holds no chart named {}; a selection naming a chart that is not there would \
                 check nothing and pass",
                charts.display(),
                unknown.join(", ")
            )));
        }
        Ok(every
            .into_iter()
            .filter(|dir| chart_name(dir).is_some_and(|name| self.admits(name)))
            .collect())
    }

    /// [`declared`], over the selected charts only.
    ///
    /// # Errors
    /// As [`Selection::dirs`], and [`Error::Invalid`] when a selected chart's declaration cannot be
    /// read as one.
    pub fn declared(
        &self,
        charts: &Path,
        documents_only: bool,
    ) -> Result<Vec<(PathBuf, Declaration)>, Error> {
        declared_in(self.dirs(charts)?, documents_only)
    }
}

/// A chart directory's name, which is the chart's name.
fn chart_name(chart_dir: &Path) -> Option<&str> {
    chart_dir.file_name().and_then(std::ffi::OsStr::to_str)
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
    declared_in(chart_dirs(charts)?, documents_only)
}

/// [`declared`], over the given chart directories.
fn declared_in(
    chart_dirs: Vec<PathBuf>,
    documents_only: bool,
) -> Result<Vec<(PathBuf, Declaration)>, Error> {
    let mut found = Vec::new();
    for chart_dir in chart_dirs {
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
        normalized: normalized_image(registry, repository),
        digest: tag.split_once('@').map(|(_, digest)| digest.to_owned()),
    })
}

/// A chart's spelling of an image, in the fully qualified form a registry client and a signature
/// check agree on.
///
/// A chart writes a bare Docker Hub name because that is what a templating engine and a container
/// CLI accept; a registry client reads a bare first segment as a hostname and tries to resolve it by
/// DNS. Signature verification normalises and the registry client does not, so passing the chart's
/// spelling around would have the two disagree about which image was verified.
///
/// The rule is the container runtime's own: with no `registry`, a first segment that contains a
/// `.` or a `:`, or is `localhost`, already names the registry and is kept; anything else is Docker
/// Hub, where a single-segment name lives under `library/`.
#[must_use]
pub fn normalized_image(registry: &str, repository: &str) -> String {
    if !registry.is_empty() {
        return format!("{registry}/{repository}");
    }
    match repository.split_once('/') {
        Some((host, _)) if host.contains(['.', ':']) || host == "localhost" => {
            repository.to_owned()
        }
        Some(_) => format!("docker.io/{repository}"),
        None => format!("docker.io/library/{repository}"),
    }
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

/// One chart's documents, resolved to the contracts that describe them.
///
/// Built **without** the staleness interlock [`bind`] applies, and deliberately so: a gate that
/// refuses to run during a digest bump withdraws its report from the one pull request it exists
/// for. Whether the vendored copy is for the digest the chart currently pins is a separate question
/// from what the file says, and every reader of this answers it for itself.
///
/// `releases` is carried beside `unions` because the merge drops it: a union is what every reader
/// of one document agrees the keys *are*, and which release each reader was built at is not part of
/// that. It is still the answer to a question one rule has to ask — a chart whose services are
/// separate images moves them one at a time, so "has the thing I transcribed moved?" is per image
/// and not per chart.
#[derive(Debug, Clone)]
pub struct Bound {
    /// The chart's directory name.
    pub chart: String,
    /// What it declared.
    pub declaration: Declaration,
    /// Every declared document, by name.
    pub documents: BTreeMap<String, Document>,
    /// Each document's contracts, merged.
    pub unions: BTreeMap<String, Union>,
    /// The releases each document's contracts were published at, sorted and deduplicated.
    pub releases: BTreeMap<String, Vec<String>>,
}

impl Bound {
    /// Resolve one chart's declaration to the contracts it names.
    ///
    /// # Errors
    /// [`Error::Invalid`] when a vendored contract cannot be read, or two cannot be reconciled.
    pub fn of(chart_dir: &Path, declaration: Declaration) -> Result<Self, Error> {
        let mut documents = BTreeMap::new();
        let mut unions = BTreeMap::new();
        let mut releases = BTreeMap::new();

        for document in &declaration.documents {
            let loaded = vendored_for(chart_dir, document)?;
            let contracts: Vec<(String, Json)> = loaded
                .iter()
                .map(|item| (item.label.clone(), item.vendored.contract.clone()))
                .collect();
            let mut published: Vec<String> = loaded
                .iter()
                .map(|item| item.vendored.published_version().to_owned())
                .collect();
            published.sort();
            published.dedup();

            unions.insert(document.name.clone(), union_contracts(&contracts)?);
            releases.insert(document.name.clone(), published);
            documents.insert(document.name.clone(), document.clone());
        }

        Ok(Self {
            chart: declaration.chart.clone(),
            declaration,
            documents,
            unions,
            releases,
        })
    }

    /// The half of one document's contract a marker of this class may name.
    pub fn namespace(&self, document: &str, targets_a_key: bool) -> Option<&Ordered> {
        let union = self.unions.get(document)?;
        Some(if targets_a_key {
            &union.keys
        } else {
            &union.external_env
        })
    }
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
    /// The same images as typed documents, by digest, for the rules a union does not carry —
    /// whether the image applies a change without restarting is one. An image whose document this
    /// build cannot type is absent, which every reader of this treats as declaring nothing.
    pub contracts: BTreeMap<String, crate::document::Contract>,
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
    let mut typed: BTreeMap<String, crate::document::Contract> = BTreeMap::new();

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
        if let Ok(contract) =
            crate::document::Contract::from_json(&item.vendored.contract.to_string())
        {
            typed.insert(digest.clone(), contract);
        }
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

    Ok((
        Some(Binding {
            union,
            by_digest,
            contracts: typed,
        }),
        Vec::new(),
    ))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::{
        Consumers, Declaration, Selection, load_declaration, parse_declaration, read_yaml,
        resolve_image, schema_violations,
    };
    use crate::gate::DocumentFormat;

    /// One declaration read as `charts/app`'s, with nothing on disk.
    fn parsed(body: &str) -> Result<Declaration, crate::error::Error> {
        parse_declaration(body, Path::new("charts/app"))
    }

    fn refused(body: &str) -> String {
        parsed(body)
            .expect_err("the declaration is refused")
            .to_string()
    }

    /// Every key the reader accepts, at one level of the file, against the schema's for that level.
    fn schema_keys(pointer: &str) -> Vec<String> {
        let schema: serde_json::Value =
            serde_json::from_str(super::DECLARATION_SCHEMA).expect("the schema is JSON");
        let mut keys: Vec<String> = schema
            .pointer(pointer)
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("the schema has {pointer}"))
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    fn sorted(keys: &[&str]) -> Vec<String> {
        let mut held: Vec<String> = keys.iter().map(|key| (*key).to_owned()).collect();
        held.sort();
        held
    }

    #[test]
    fn the_schema_names_exactly_the_keys_the_reader_accepts() {
        use super::{
            CONSUMER_KEYS, CREDENTIAL_KEYS, DECLARATION_KEYS, DOCUMENT_KEYS, EXEMPT_KEYS,
            IMAGE_KEYS, RESTART_KEYS, SOURCE_KEYS, UNBOUND_KEYS,
        };
        use crate::helm::suites::{ENTRY_KEYS, PREREQUISITE_KEYS, UNROUTED_KEYS};

        let levels: [(&str, &[&str]); 12] = [
            ("/properties", &DECLARATION_KEYS),
            ("/$defs/document/properties", &DOCUMENT_KEYS),
            ("/$defs/source/properties", &SOURCE_KEYS),
            ("/$defs/image/properties", &IMAGE_KEYS),
            ("/$defs/consumer/properties", &CONSUMER_KEYS),
            (
                "/$defs/consumer/properties/workload/properties",
                &["kind", "selector"],
            ),
            ("/$defs/exemption/properties", &EXEMPT_KEYS),
            ("/$defs/restart/properties", &RESTART_KEYS),
            ("/$defs/unbound/properties", &UNBOUND_KEYS),
            ("/$defs/credential/properties", &CREDENTIAL_KEYS),
            ("/$defs/tests/properties", &ENTRY_KEYS),
            (
                "/$defs/tests/properties/unrouted/items/properties",
                &UNROUTED_KEYS,
            ),
        ];
        for (pointer, keys) in levels {
            assert_eq!(schema_keys(pointer), sorted(keys), "{pointer}");
        }
        assert_eq!(
            schema_keys("/$defs/tests/properties/prerequisites/properties"),
            sorted(&PREREQUISITE_KEYS)
        );

        let gates: Vec<&str> = crate::gate::Relaxed::GATES
            .iter()
            .map(|(gate, _)| *gate)
            .collect();
        let schema: serde_json::Value =
            serde_json::from_str(super::DECLARATION_SCHEMA).expect("the schema is JSON");
        assert_eq!(
            schema["$defs"]["exemption"]["properties"]["gates"]["items"]["enum"],
            json!(gates)
        );
    }

    #[test]
    fn the_schema_accepts_what_the_reader_accepts() {
        let accepted = [
            "documents:\n  - name: app\n",
            "documents: []\nreason: renders no configuration\n",
            "\
bindings: true
tests: true
unconfigured: [sidecar.image]
unbound:
  - keys: [a.b]
    reason: delivered as a file
    documents: [app]
credentials:
  - key: a.b
    value: existingSecret
documents:
  - name: app
    source:
      kind: Secret
      selector: { app: x }
      key: settings.json
      format: json
    images:
      - values: image
        contract: contracts/app.json
      - values: other.image
        contract: contracts/other.json
    consumers:
      - workload: { kind: Deployment, selector: { app: x } }
        containers: [app]
    exempt:
      - values: ci/minimal.yaml
        gates: [env, files]
        reason: renders no workload
    restart:
      - keys: [a.c]
        reason: canaried
    tests:
      probe: app.config
      baseline: { app.config.a.d: false }
      reason: the pair is refused together
      prerequisites:
        values: { existingSecret: probe }
        reason: the chart refuses to render without one
      unrouted:
        - keys: [a.e]
          reason: guarded
",
        ];
        for body in accepted {
            parsed(body).expect("the reader accepts it");
            assert_eq!(schema_violations(body), Vec::<String>::new(), "{body}");
        }
    }

    #[test]
    fn the_schema_refuses_what_the_reader_refuses_by_shape() {
        let refused_both = [
            "documents: []\n",
            "documents:\n  - name: app\n    typo: 1\n",
            "documents:\n  - name: app\n    source: { selector: {} }\n",
            "documents:\n  - name: app\n    images: []\n",
            "documents:\n  - name: app\n    source: { format: ini }\n",
            "documents:\n  - name: app\n    exempt:\n      - { gates: [all], reason: x }\n",
            "bindings: yes please\ndocuments:\n  - name: app\n",
            "credentials:\n  - key: a\ndocuments:\n  - name: app\n",
            "tests: true\ndocuments:\n  - name: app\n    tests: { probe: app.x }\n",
        ];
        for body in refused_both {
            parsed(body).expect_err(body);
            assert_ne!(schema_violations(body), Vec::<String>::new(), "{body}");
        }
    }

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

    /// A chart tree holding `alpha`, `beta` and `gamma`, and a directory that is not a chart.
    fn three_charts() -> tempdir::Dir {
        let tree = tempdir::Dir::new();
        for name in ["alpha", "beta", "gamma"] {
            let chart = tree.path().join(name);
            std::fs::create_dir_all(&chart).expect("the chart directory is created");
            std::fs::write(chart.join("Chart.yaml"), format!("name: {name}\n"))
                .expect("the chart is written");
        }
        std::fs::create_dir_all(tree.path().join("scripts")).expect("the directory is created");
        tree
    }

    fn names(dirs: &[std::path::PathBuf]) -> Vec<&str> {
        dirs.iter()
            .filter_map(|dir| dir.file_name().and_then(std::ffi::OsStr::to_str))
            .collect()
    }

    #[test]
    fn a_selection_naming_nothing_is_the_whole_tree() {
        let tree = three_charts();
        let dirs = Selection::all()
            .dirs(tree.path())
            .expect("the tree is walked");
        assert_eq!(names(&dirs), ["alpha", "beta", "gamma"]);
        assert_eq!(Selection::of(Vec::<String>::new()), Selection::all());
    }

    #[test]
    fn a_selection_reads_only_the_charts_it_names_in_tree_order() {
        let tree = three_charts();
        let dirs = Selection::of(["gamma", "alpha"])
            .dirs(tree.path())
            .expect("both charts exist");
        assert_eq!(names(&dirs), ["alpha", "gamma"]);
    }

    #[test]
    fn a_selection_naming_a_chart_the_tree_lacks_is_refused_rather_than_empty() {
        // Otherwise a typo in a pipeline selects nothing, and a gate over nothing passes.
        let tree = three_charts();
        let failure = Selection::of(["alpha", "delta", "scripts"])
            .dirs(tree.path())
            .expect_err("neither `delta` nor `scripts` is a chart")
            .to_string();
        assert!(
            failure.contains("holds no chart named delta, scripts"),
            "{failure}"
        );
    }

    #[test]
    fn a_selected_walk_never_reads_a_chart_outside_it() {
        // `beta` carries a declaration that cannot be parsed; selecting around it must not fail.
        let tree = three_charts();
        std::fs::write(
            tree.path().join("beta").join("config-contract.yaml"),
            "documents: [\n",
        )
        .expect("the broken declaration is written");

        Selection::all()
            .declared(tree.path(), false)
            .expect_err("the whole tree reads `beta`");
        let found = Selection::of(["alpha", "gamma"])
            .declared(tree.path(), false)
            .expect("`beta` is not read");
        assert!(found.is_empty(), "neither selected chart declares anything");
    }

    // -- defaults --------------------------------------------------------------------------------

    #[test]
    fn a_document_naming_only_itself_takes_every_default() {
        let declaration = parsed("documents:\n  - name: server\n").expect("it reads");
        let document = &declaration.documents[0];

        assert_eq!(document.source.kind, "ConfigMap");
        assert_eq!(
            document.source.selector,
            vec![("app.kubernetes.io/instance".to_owned(), "app".to_owned())]
        );
        assert_eq!(document.source.key, "config.toml");
        assert_eq!(document.source.format, DocumentFormat::Toml);
        assert_eq!(document.images.len(), 1);
        assert_eq!(document.images[0].values, "image");
        assert_eq!(document.images[0].contract, "contracts/server.json");
        assert!(document.consumers.inferred());
        assert_eq!(
            document.defaulted,
            vec![
                "source.kind",
                "source.selector",
                "source.key",
                "source.format",
                "images",
                "consumers",
            ]
        );
    }

    #[test]
    fn every_written_field_wins_over_its_default() {
        let declaration = parsed(
            "documents:\n  - name: server\n    source:\n      kind: Secret\n      \
             selector: {app.kubernetes.io/component: api}\n      key: settings.json\n      \
             format: yaml\n    images:\n      - {values: services.api.image, contract: c/a.json}\n    \
             consumers:\n      - workload: {kind: Job}\n        containers: [api]\n",
        )
        .expect("it reads");
        let document = &declaration.documents[0];

        assert_eq!(document.source.kind, "Secret");
        assert_eq!(
            document.source.selector,
            vec![("app.kubernetes.io/component".to_owned(), "api".to_owned())]
        );
        assert_eq!(document.source.key, "settings.json");
        assert_eq!(
            document.source.format,
            DocumentFormat::Yaml,
            "written beats the extension"
        );
        assert_eq!(document.images[0].values, "services.api.image");
        assert_eq!(document.images[0].contract, "c/a.json");
        assert_eq!(document.consumers.declared().len(), 1);
        assert!(document.defaulted.is_empty(), "{:?}", document.defaulted);
    }

    #[test]
    fn the_format_follows_the_keys_extension_before_it_falls_back_to_toml() {
        for (key, format) in [
            ("config.toml", DocumentFormat::Toml),
            ("config.json", DocumentFormat::Json),
            ("config.yaml", DocumentFormat::Yaml),
            ("config.YML", DocumentFormat::Yaml),
            ("config", DocumentFormat::Toml),
            ("settings.ini", DocumentFormat::Toml),
        ] {
            let declaration = parsed(&format!(
                "documents:\n  - name: server\n    source: {{key: {key}}}\n"
            ))
            .expect("it reads");
            assert_eq!(declaration.documents[0].source.format, format, "{key}");
        }
    }

    #[test]
    fn a_contract_is_defaulted_only_for_a_documents_one_image() {
        let declaration =
            parsed("documents:\n  - name: server\n    images: [{values: app.image}]\n")
                .expect("one image may leave its contract out");
        assert_eq!(
            declaration.documents[0].images[0].contract,
            "contracts/server.json"
        );
        assert!(
            declaration.documents[0]
                .defaulted
                .contains(&"images[].contract")
        );

        let error = refused(
            "documents:\n  - name: server\n    images:\n      - {values: a.image}\n      \
             - {values: b.image, contract: contracts/b.json}\n",
        );
        assert!(
            error.contains("only when the document has one image"),
            "{error}"
        );
    }

    #[test]
    fn an_empty_selector_is_refused_rather_than_matching_every_object() {
        let error = refused("documents:\n  - name: server\n    source: {selector: {}}\n");
        assert!(error.contains("non-empty mapping of labels"), "{error}");
        assert!(error.contains("app.kubernetes.io/instance: app"), "{error}");
    }

    #[test]
    fn an_empty_images_list_is_refused_rather_than_read_as_the_default() {
        let error = refused("documents:\n  - name: server\n    images: []\n");
        assert!(error.contains("non-empty list"), "{error}");
    }

    #[test]
    fn consumers_are_inferred_only_for_an_object_a_pod_mounts_by_name() {
        let error = refused("documents:\n  - name: server\n    source: {kind: Settings}\n");
        assert!(error.contains("`consumers` is missing"), "{error}");

        let declaration = parsed("documents:\n  - name: server\n    consumers: []\n")
            .expect("an empty list is a written decision");
        assert!(matches!(
            &declaration.documents[0].consumers,
            Consumers::Declared(listed) if listed.is_empty()
        ));
    }

    #[test]
    fn a_document_declared_twice_is_refused() {
        let error = refused("documents:\n  - name: server\n  - name: server\n");
        assert!(error.contains("declared twice"), "{error}");
    }

    #[test]
    fn inferred_consumers_are_the_workloads_mounting_the_selected_object() {
        let declaration = parsed("documents:\n  - name: server\n").expect("it reads");
        let document = &declaration.documents[0];
        let manifests = vec![
            json!({"kind": "ConfigMap", "metadata": {
                "name": "renamed", "labels": {"app.kubernetes.io/instance": "app"},
            }}),
            json!({"kind": "ConfigMap", "metadata": {
                "name": "other", "labels": {"app.kubernetes.io/instance": "elsewhere"},
            }}),
            json!({"kind": "Deployment", "metadata": {"name": "app"}, "spec": {"template": {
                "spec": {
                    "volumes": [{"name": "config", "configMap": {"name": "renamed"}}],
                    "containers": [
                        {"name": "app", "volumeMounts": [{"name": "config", "mountPath": "/c"}]},
                    ],
                },
            }}}),
            json!({"kind": "Deployment", "metadata": {"name": "unrelated"}, "spec": {"template": {
                "spec": {
                    "volumes": [{"name": "config", "configMap": {"name": "other"}}],
                    "containers": [
                        {"name": "x", "volumeMounts": [{"name": "config", "mountPath": "/c"}]},
                    ],
                },
            }}}),
        ];

        let consumers = document.consumers_in(&manifests);
        assert_eq!(consumers.len(), 1);
        assert_eq!(consumers[0].kind, "Deployment");
        assert_eq!(consumers[0].name.as_deref(), Some("app"));
        assert_eq!(consumers[0].containers, vec!["app".to_owned()]);
        let selected = consumers[0].select(&manifests);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0]["metadata"]["name"], "app");

        assert!(
            document.consumers_in(&manifests[1..]).is_empty(),
            "no object selected, so nothing mounts it"
        );
    }

    // -- round-trip enrolment ---------------------------------------------------------------------

    #[test]
    fn an_enrolled_chart_gives_every_document_a_suite_and_its_block_says_how() {
        let declaration = parsed(
            "tests: true\ndocuments:\n  - name: server\n    tests:\n      prerequisites:\n        \
             values: {existingSecret: probe}\n        reason: the render guard wants a secret\n  \
             - name: worker\n",
        )
        .expect("it reads");
        assert!(declaration.tests);
        let server = declaration.documents[0].tests.as_ref().expect("enrolled");
        assert_eq!(
            server.prerequisites.values,
            vec![("existingSecret".to_owned(), json!("probe"))]
        );
        let worker = declaration.documents[1]
            .tests
            .as_ref()
            .expect("enrolled by default");
        assert!(
            worker.prerequisites.values.is_empty(),
            "a document with no block takes the defaults"
        );

        let unenrolled = parsed("documents:\n  - name: server\n").expect("it reads");
        assert!(!unenrolled.tests);
        assert!(unenrolled.documents[0].tests.is_none());
    }

    #[test]
    fn a_tests_block_on_a_chart_that_is_not_enrolled_is_refused() {
        let error = refused(
            "documents:\n  - name: server\n    tests:\n      probe: config\n      reason: x\n",
        );
        assert!(error.contains("set `tests: true`"), "{error}");
    }

    #[test]
    fn a_tests_block_is_read_under_the_enrolment_files_rules() {
        let error = refused("tests: true\ndocuments:\n  - name: server\n    tests: {name: x}\n");
        assert!(error.contains("unknown key(s) name"), "{error}");

        let error = refused(
            "tests: true\ndocuments:\n  - name: server\n    tests:\n      \
             baseline: {config.a: 1}\n",
        );
        assert!(error.contains("has no `reason`"), "{error}");

        let error = refused("tests: yes please\ndocuments:\n  - name: server\n");
        assert!(error.contains("must be true or false"), "{error}");
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
        assert_eq!(pinned.normalized, "ghcr.io/x/y");
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
