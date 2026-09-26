//! The contract document, as a consumer reads it.
//!
//! A mirror of `spec/v1/contract.schema.json`, and deliberately not a mirror of any producer's
//! types. The Rust implementation's `Contract` is a *writer*: it is built from Rust types, it
//! refuses at build time, and every field it has is one it knows how to fill. This is the other
//! direction, and the difference is not cosmetic — a reader that is as strict as a writer refuses
//! documents it should have read.
//!
//! # Read tolerantly, gate on the envelope
//!
//! Two rules, both from `spec/v1/FORMAT.md`:
//!
//! **Unknown fields are kept, not refused.** `serde`'s default is to ignore them, which is what
//! this wants: a producer emitting a field this build has not learned is a producer that is ahead,
//! not a producer that is wrong. The envelope version is the field that says whether the document
//! is readable at all, and it is checked explicitly rather than by the shape parsing.
//!
//! **`terrace_contract` is checked before anything else is believed.** A version this build was
//! not written against is refused with the number in the message, because misreading a document is
//! worse than declining to read it. `schema_version` is the second gate and a softer one: a
//! *higher* one is readable, since the schema half only ever gained fields, but a consumer walking
//! `constraint`'s keywords itself has to know it is looking at a shape it may not fully understand.
//!
//! # What is not modelled
//!
//! `constraint`, `text_constraint` and `json_schema` stay [`Json`]. They are JSON Schema, whose
//! vocabulary is open by design and which this crate hands to a validator rather than interpreting
//! keyword by keyword. Modelling them would mean a struct that silently drops the keyword a
//! producer added last week — which is the failure `FORMAT.md` warns about in its own words: "a
//! catch-all on purpose. Enumerating the fields that matter means the next field the producer adds
//! falls through the gap in silence."

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::Error;

/// The envelope version this build reads.
///
/// A document declaring anything else is refused by [`Contract::from_json`]. Bumped only when a
/// change lands that a consumer written against the old shape cannot read.
pub const CONTRACT_VERSION: u32 = 1;

/// The highest `schema.schema_version` this build understands completely.
///
/// A document declaring a higher one is still read — the schema half has only ever gained fields —
/// but [`Contract::schema_version_ahead`] reports it, so a caller walking `constraint` itself can
/// say "this document may carry more than I know about" rather than quietly under-reading it.
///
/// Version 3 added `propertyNames`, which a producer's entry-name refinement writes. Version 4 added
/// the conditions between a struct's fields: `oneOf`, `if`, `then`, `minProperties` and
/// `maxProperties`, beside `allOf`, `anyOf`, `not` and `const`. A producer publishes the lowest
/// version whose vocabulary holds what the document carries, so most documents still declare 2.
pub const SCHEMA_VERSION: u32 = 4;

/// The default in-image path a contract is published at.
///
/// The value of the `dev.terrace.config.contract.path` label unless a build says otherwise.
pub const DEFAULT_PATH: &str = "/config/contract.json";

/// The label naming the envelope version.
pub const LABEL_VERSION: &str = "dev.terrace.config.contract.version";
/// The label naming where the document is inside the image.
pub const LABEL_PATH: &str = "dev.terrace.config.contract.path";
/// The label naming the loader's environment prefix.
pub const LABEL_PREFIX: &str = "dev.terrace.config.prefix";

/// The marker opening a generated `LABEL` block in a Dockerfile.
pub const MARKER_BEGIN: &str = "# terrace-config:labels:begin";
/// The marker closing it.
pub const MARKER_END: &str = "# terrace-config:labels:end";

/// One image's configuration contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contract {
    /// The version of this envelope's shape.
    pub terrace_contract: u32,
    /// Which implementation wrote this document, and which loader it describes.
    ///
    /// Optional here and required by `spec/v1/contract.schema.json`, and the asymmetry is
    /// deliberate. The field was added *within* envelope 1, so documents predating it are legal
    /// `terrace_contract: 1` bytes that a reader as strict as the writer would refuse — and every
    /// contract vendored by the chart repository this consumer half was ported against is one of
    /// them. A reader that refuses a document it should have read is the failure this module opens
    /// by naming. `conform` and `validate` still require it, through the meta-schema, which is
    /// where "this producer is not emitting what it must" belongs.
    ///
    /// [`None`] is not the same as absent to a consumer, either: it means the loader whose
    /// environment reads every `text_constraint` here was measured against is unknown, so the range
    /// check must be skipped and reported as skipped. See [`crate::value`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<Producer>,
    /// Which build this describes.
    pub app: App,
    /// Every key the loader can carry, in every spelling that can supply it.
    pub schema: Schema,
    /// The same keys as a JSON Schema, for validating the document a chart renders.
    #[serde(default)]
    pub json_schema: Json,
    /// The surface outside the loader's namespace.
    #[serde(default)]
    pub external: External,
}

/// Which implementation wrote a [`Contract`], and which loader its text constraints describe.
///
/// `loader` is the load-bearing field and the reason this is required rather than optional: the
/// `text_constraint` patterns were *measured* against one library's environment reads, and
/// applying one loader's reads to another's document is wrong in the direction that costs a
/// deployment. A consumer that cannot name the loader must not perform the range check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Producer {
    /// The implementation's own name, e.g. `terrace-config`.
    pub name: String,
    /// Its version, or a sentinel in a conformance document.
    pub version: String,
    /// The library whose environment reads the text constraints were measured against.
    pub loader: String,
}

/// Which build a contract describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct App {
    /// The application's name.
    pub name: String,
    /// The version, ideally the one the image tag carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The commit the build came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// When the build happened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Where the source lives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Every key a configuration can carry, and how the loader spells them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    /// The version of this half's shape.
    pub schema_version: u32,
    /// How keys are spelled in the environment.
    pub dialect: Dialect,
    /// The variables the loader reads before the layers exist.
    #[serde(default)]
    pub loader: Vec<LoaderVar>,
    /// Whether the image applies a change without restarting. [`None`] is undeclared, which
    /// [`crate::reload`] reads as "no rebuild" and reports as undeclared rather than as `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reload: Option<ReloadSupport>,
    /// Every key, in declaration order.
    #[serde(default)]
    pub keys: Vec<Key>,
}

/// Whether an image applies a configuration change without restarting, and through which layers.
///
/// A fact about the binary, not about `producer.loader`. What a consumer does with it lives in
/// [`crate::reload`]; this is the shape alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReloadSupport {
    /// How a change is applied.
    pub mode: ReloadMode,
    /// The file layers watched for one.
    #[serde(default)]
    pub layers: Vec<ReloadLayer>,
}

/// How an image applies a change after start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReloadMode {
    /// Nothing is applied after start.
    None,
    /// A change re-reads every layer and rebuilds the runtime, keeping `restart` keys pinned.
    Rebuild,
    /// A mode a later envelope names. Read as [`Self::None`], and reported as degraded.
    #[serde(other)]
    Other,
}

impl ReloadMode {
    /// The spelling a contract publishes.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Rebuild => "rebuild",
            Self::Other => "other",
        }
    }
}

/// One file layer a rebuilding image watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReloadLayer {
    /// The TOML document layer.
    Document,
    /// The directory of key-named files.
    SecretsDir,
    /// The files indirection variables name.
    EnvFile,
    /// A layer a later envelope names, which no channel this build knows is delivered through.
    #[serde(other)]
    Other,
}

impl ReloadLayer {
    /// The spelling a contract publishes.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::SecretsDir => "secrets_dir",
            Self::EnvFile => "env_file",
            Self::Other => "other",
        }
    }
}

/// Whether a rebuild applies a change to one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Reload {
    /// A rebuild applies it.
    Live,
    /// Only a process start applies it.
    Restart,
    /// A class a later envelope names. Read as [`Self::Restart`], and reported as degraded.
    #[serde(other)]
    Other,
}

impl Reload {
    /// The spelling a contract publishes.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Restart => "restart",
            Self::Other => "other",
        }
    }
}

/// How a key path becomes an environment spelling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dialect {
    /// The namespace every spelling starts with.
    pub prefix: String,
    /// What separates one path segment from the next.
    pub nesting_separator: String,
    /// What is appended to name a file holding the value instead.
    pub indirection_suffix: String,
}

/// A variable the loader reads to decide what the layers are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoaderVar {
    /// The variable's spelling.
    pub env: String,
    /// What it selects.
    pub role: LoaderRole,
    /// The producer's prose about it.
    #[serde(default)]
    pub docs: String,
    /// What the loader assumes when it is unset.
    #[serde(default)]
    pub default: Option<String>,
}

/// What a loader variable selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LoaderRole {
    /// Names the TOML layer.
    Config,
    /// Names a directory of key-named files.
    SecretsDir,
    /// Read directly from the environment, so no file may supply it.
    Reserved,
    /// A role a later envelope names and this build has none for.
    ///
    /// A *new* variant rather than folding into one of the three above, and the distinction is
    /// load-bearing. Every role means "the loader reads this variable", so the ordered list's step
    /// 1 is satisfied by `env` alone whatever this says — but a consumer looking for the secrets
    /// directory matches on the role, and an unknown one read as [`Self::Config`] would hand it
    /// the wrong variable.
    #[serde(other)]
    Other,
}

impl LoaderRole {
    /// The spelling a rendered table shows.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::SecretsDir => "secrets dir",
            Self::Reserved => "reserved",
            Self::Other => "other",
        }
    }
}

/// One configuration key, in every spelling that can supply it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Key {
    /// The TOML path.
    pub path: String,
    /// The variable supplying the value directly, or `None` when none can.
    #[serde(default)]
    pub env: Option<String>,
    /// The variable naming a file holding the value.
    #[serde(default)]
    pub env_file: Option<String>,
    /// The file name inside the secrets directory.
    #[serde(default)]
    pub secrets_file: Option<String>,
    /// The producer's `///` comment, whole.
    #[serde(default)]
    pub docs: String,
    /// The producer's own name for the type. **Not portable** — see [`Key::ty`].
    #[serde(default)]
    pub ty: Option<String>,
    /// The choices, when the type is a closed set.
    #[serde(default)]
    pub values: Vec<String>,
    /// What the parsed value must satisfy.
    #[serde(default)]
    pub constraint: Option<Json>,
    /// What the raw characters of an environment variable must satisfy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_constraint: Option<Json>,
    /// How to read the text, for the step a pattern cannot perform.
    #[serde(default)]
    pub text_form: TextForm,
    /// Other paths that supply the same key.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Other variables that supply it.
    #[serde(default)]
    pub env_aliases: Vec<String>,
    /// Other indirection variables.
    #[serde(default)]
    pub env_file_aliases: Vec<String>,
    /// Other secrets-directory file names.
    #[serde(default)]
    pub secrets_file_aliases: Vec<String>,
    // Placed here, after the alias lists rather than beside `secrets_file` where it reads
    // best, because declaration order *is* wire order under `serde` and the wire order is the
    // producer's. A round trip that reordered a field would make `stamp` rewrite bytes nobody
    // asked it to.
    /// Why no variable names this key, when none does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreachable: Option<Unreachable>,
    /// The default as a table would show it.
    #[serde(default)]
    pub default: Option<String>,
    /// The same default, as the value itself.
    #[serde(default)]
    pub default_value: Option<Json>,
    /// The producer's prose about the default.
    #[serde(default)]
    pub note: Option<String>,
    /// Whether some layer must supply it.
    #[serde(default)]
    pub required: bool,
    /// Whether the value is a credential.
    #[serde(default)]
    pub secret: bool,
    /// Whether the loader reads it before the layers exist.
    #[serde(default)]
    pub reserved: bool,
    /// Whether a rebuild applies a change to it. [`None`] is undeclared; see [`crate::reload`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reload: Option<Reload>,
}

impl Key {
    /// The entries this key's map must contain, as its constraint publishes them.
    ///
    /// See [`required_entries`]. A method so that every renderer reads the one source the
    /// validators read, rather than a field of its own that could drift from it.
    pub fn required_entries(&self) -> Vec<&str> {
        required_entries(self.constraint.as_ref())
    }

    /// The pattern this key's map holds its entry names to, as its constraint publishes it.
    ///
    /// See [`entry_name_pattern`].
    pub fn entry_name_pattern(&self) -> Option<&str> {
        entry_name_pattern(self.constraint.as_ref())
    }

    /// Every tightening this key's constraint publishes, with its position inside the key.
    ///
    /// See [`tightenings`].
    pub fn tightenings(&self) -> Vec<(String, Tightening<'_>)> {
        tightenings(self.constraint.as_ref())
    }
}

/// How deep [`tightenings`] follows a constraint before it stops: a document is untrusted input.
const MAX_TIGHTENING_DEPTH: usize = 32;

/// One tightening `FORMAT.md` (*Refinements*) lets a producer publish, as a rendering names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tightening<'a> {
    /// A map's `required` entries.
    Entries(Vec<&'a str>),
    /// A map's `propertyNames` pattern.
    Names(&'a str),
    /// A string's `pattern`.
    Matches(&'a str),
    /// A struct's condition, by the sentence its `allOf` member carries as `description`.
    Holds(&'a str),
}

impl Tightening<'_> {
    /// The keyword the tightening is published under.
    pub const fn keyword(&self) -> &'static str {
        match self {
            Self::Entries(_) => "required",
            Self::Names(_) => "propertyNames",
            Self::Matches(_) => "pattern",
            Self::Holds(_) => "allOf",
        }
    }
}

/// Every tightening a constraint publishes, each with its position relative to the key.
///
/// The position is empty for the key itself and `*.body` for the `body` field of every element,
/// `*` standing for an element as the producer's refinement path spells it.
///
/// In the order a reader meets them, which is the order every rendering prints them in: the key's
/// own — entries, entry names, pattern — then its element, then its fields as the constraint lists
/// them. A struct's `required` names its fields rather than a map's entries and is not one; neither
/// is a `propertyNames` other than a lone pattern.
pub fn tightenings(constraint: Option<&Json>) -> Vec<(String, Tightening<'_>)> {
    let mut found = Vec::new();
    if let Some(constraint) = constraint {
        walk_tightenings(constraint, "", 0, &mut found);
    }
    found
}

fn walk_tightenings<'a>(
    schema: &'a Json,
    at: &str,
    depth: usize,
    found: &mut Vec<(String, Tightening<'a>)>,
) {
    let Some(schema) = schema.as_object() else {
        return;
    };
    if depth > MAX_TIGHTENING_DEPTH {
        return;
    }
    let entries = required_entries_of(schema);
    if !entries.is_empty() {
        found.push((at.to_owned(), Tightening::Entries(entries)));
    }
    if let Some(pattern) = entry_name_pattern_of(schema) {
        found.push((at.to_owned(), Tightening::Names(pattern)));
    }
    if let Some(pattern) = schema.get("pattern").and_then(Json::as_str) {
        found.push((at.to_owned(), Tightening::Matches(pattern)));
    }
    for description in conditions_of(schema) {
        found.push((at.to_owned(), Tightening::Holds(description)));
    }

    let below = |segment: &str| {
        if at.is_empty() {
            segment.to_owned()
        } else {
            format!("{at}.{segment}")
        }
    };
    for keyword in ["additionalProperties", "items"] {
        if let Some(element) = schema.get(keyword).filter(|element| element.is_object()) {
            walk_tightenings(element, &below("*"), depth + 1, found);
        }
    }
    if let Some(fields) = schema.get("properties").and_then(Json::as_object) {
        for (name, field) in fields {
            walk_tightenings(field, &below(name), depth + 1, found);
        }
    }
}

/// The entries a map-typed constraint requires: `required` at the top of an object that declares
/// no `properties`.
///
/// `FORMAT.md` (*Refinements*): a producer MAY tighten a map-typed key's constraint with the entry
/// names the map must contain, and a consumer showing them reads them from here. An object that
/// *does* declare `properties` is a struct, whose `required` names its fields rather than a map's
/// entries, and yields nothing — as does anything that is not an object at all.
pub fn required_entries(constraint: Option<&Json>) -> Vec<&str> {
    match constraint {
        Some(Json::Object(constraint)) => required_entries_of(constraint),
        _ => Vec::new(),
    }
}

/// [`required_entries`], over a constraint already known to be an object.
pub fn required_entries_of(constraint: &serde_json::Map<String, Json>) -> Vec<&str> {
    if constraint.get("type").and_then(Json::as_str) != Some("object")
        || constraint.contains_key("properties")
    {
        return Vec::new();
    }
    constraint
        .get("required")
        .and_then(Json::as_array)
        .map(|names| names.iter().filter_map(Json::as_str).collect())
        .unwrap_or_default()
}

/// The pattern a map-typed constraint holds its entry names to: `propertyNames: {"pattern": …}` at
/// the top of an object that declares no `properties`.
///
/// `FORMAT.md` (*Refinements*): from `schema_version: 3` a producer MAY hold a map's entry names to a
/// pattern from the portable subset, and a consumer showing it reads it from here. Anything else
/// under `propertyNames` is not a refinement this format defines and yields nothing; the validators
/// still apply it.
pub fn entry_name_pattern(constraint: Option<&Json>) -> Option<&str> {
    match constraint {
        Some(Json::Object(constraint)) => entry_name_pattern_of(constraint),
        _ => None,
    }
}

/// [`entry_name_pattern`], over a constraint already known to be an object.
pub fn entry_name_pattern_of(constraint: &serde_json::Map<String, Json>) -> Option<&str> {
    if constraint.get("type").and_then(Json::as_str) != Some("object")
        || constraint.contains_key("properties")
    {
        return None;
    }
    let names = constraint.get("propertyNames")?.as_object()?;
    if names.len() != 1 {
        return None;
    }
    names.get("pattern")?.as_str()
}

/// The sentences of the conditions a schema carries: its `allOf` members carrying `description`.
///
/// `FORMAT.md` (*Refinements*): a condition between a struct's fields is published as one `allOf`
/// member whose `description` is its readable form. A member without one — the choice of spellings a
/// producer writes for a required field with an alias — is not a condition.
pub fn conditions_of(schema: &serde_json::Map<String, Json>) -> impl Iterator<Item = &str> {
    schema
        .get("allOf")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|member| member.get("description").and_then(Json::as_str))
}

/// Why the environment cannot name a key.
///
/// The two differ in whether the environment can reach the key at all, and a consumer meeting a
/// bare `env: null` and treating it as "skip this key" is right for the first and wrong for the
/// second — which is why the reason is published rather than inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Unreachable {
    /// No variable names it: the path does not survive the case fold, or carries the separator.
    Unnameable,
    /// A spelling collides with another key's indirection variable.
    Indirection,
    /// Anything a later envelope adds.
    #[serde(other)]
    Other,
}

/// How to read the characters an environment variable holds.
///
/// Always present, and always read rather than inferred from which keywords `text_constraint`
/// happens to carry — an inference that was right while there were two shapes and wrong the moment
/// there were three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TextForm {
    /// Any text is fine, and there is nothing to parse.
    Text,
    /// Read it as an integer.
    Integer,
    /// Read it as a boolean.
    Boolean,
    /// One of a closed set, with surrounding whitespace permitted.
    Choice,
    /// A container: the loader wants a TOML literal, not a bare list.
    Structured,
    /// Nothing could be determined. A gap, not an answer.
    #[default]
    #[serde(other)]
    Unknown,
}

impl TextForm {
    /// The spelling a contract publishes, and the one every message names it by.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Integer => "integer",
            Self::Boolean => "boolean",
            Self::Choice => "choice",
            Self::Structured => "structured",
            Self::Unknown => "unknown",
        }
    }
}

/// The surface outside the loader's namespace: what else the image reads, and what it ignores.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct External {
    /// Variables the image reads that are nobody's configuration.
    #[serde(default)]
    pub env: Vec<ExternalVar>,
    /// Patterns for variables nothing in the image reads.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// What to do about a variable no rule accounts for.
    #[serde(default)]
    pub unknown: Unknown,
}

/// One variable outside the loader's namespace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalVar {
    /// Its spelling.
    pub name: String,
    /// Who reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The prose about it.
    #[serde(default)]
    pub docs: String,
    /// The producer's name for its type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// The choices, when it takes a closed set.
    #[serde(default)]
    pub values: Vec<String>,
    /// What the parsed value must satisfy.
    #[serde(default)]
    pub constraint: Option<Json>,
    /// What the raw characters must satisfy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_constraint: Option<Json>,
    /// How to read the text.
    #[serde(default)]
    pub text_form: TextForm,
    /// What is assumed when it is unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Whether something must supply it.
    #[serde(default)]
    pub required: bool,
    /// Whether it carries a credential.
    #[serde(default)]
    pub secret: bool,
}

/// What a consumer should do about a variable no rule accounts for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Unknown {
    /// Fail the check.
    #[default]
    Reject,
    /// Report it, and do not fail.
    Warn,
    /// Say nothing.
    Ignore,
    /// Anything a later envelope adds. Treated as [`Self::Warn`], which is the safe reading.
    #[serde(other)]
    Other,
}

impl Contract {
    /// Read a document, gating on the envelope before believing anything in it.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the bytes are not JSON, or when `terrace_contract` is a version this
    /// build was not written against. A *higher* `schema_version` is not an error — see
    /// [`Self::schema_version_ahead`].
    pub fn from_json(text: &str) -> Result<Self, Error> {
        // Read the envelope version before the whole document, so a document from an envelope this
        // build cannot parse reports the version rather than a field-level deserialisation error
        // about a shape that was never going to fit.
        let probe: Envelope = serde_json::from_str(text)
            .map_err(|e| Error::Invalid(format!("not a contract document: {e}")))?;

        if probe.terrace_contract != CONTRACT_VERSION {
            return Err(Error::Invalid(format!(
                "this build reads `terrace_contract` {CONTRACT_VERSION}, and the document declares \
                 {}. Misreading a document is worse than declining to read it, so nothing further \
                 was attempted.",
                probe.terrace_contract
            )));
        }

        serde_json::from_str(text)
            .map_err(|e| Error::Invalid(format!("not a valid `terrace_contract` 1 document: {e}")))
    }

    /// The document's `schema_version`, when it is higher than this build understands.
    ///
    /// Not an error: the schema half has only ever gained fields, so a higher version is readable
    /// and every field this build knows still means what it meant. A caller that walks
    /// `constraint`'s keywords itself reports this, so "there may be more here than I checked" is
    /// something a reader is told rather than something they have to suspect.
    pub const fn schema_version_ahead(&self) -> Option<u32> {
        if self.schema.schema_version > SCHEMA_VERSION {
            Some(self.schema.schema_version)
        } else {
            None
        }
    }

    /// The three labels that make this document discoverable on an image.
    ///
    /// The label *names* live here rather than in a Dockerfile for the reason the Rust
    /// implementation gives for the same list: a Dockerfile that spells one by hand can spell it
    /// differently from the pipeline reading it, and the failure mode is a contract silently never
    /// found.
    pub fn labels(&self, path: &str) -> Vec<(&'static str, String)> {
        vec![
            (LABEL_VERSION, self.terrace_contract.to_string()),
            (LABEL_PATH, path.to_owned()),
            (LABEL_PREFIX, self.schema.dialect.prefix.clone()),
        ]
    }
}

/// Just enough of the envelope to decide whether the rest can be read.
#[derive(Deserialize)]
struct Envelope {
    terrace_contract: u32,
}

#[cfg(test)]
mod tests {
    use super::{Contract, LoaderRole, TextForm, Unknown, Unreachable};

    #[test]
    fn every_tightening_is_found_at_its_position_in_reading_order() {
        use super::Tightening;

        let constraint = serde_json::json!({
            "type": "object",
            "required": ["intro"],
            "additionalProperties": {
                "type": "object",
                "required": ["title"],
                "properties": {
                    "body": {"type": "object", "propertyNames": {"pattern": "^[a-z]{2}$"},
                             "additionalProperties": {"type": "string", "pattern": "[^ ]"}},
                    "title": {"type": "object", "required": ["en"]},
                },
            },
        });
        assert_eq!(
            super::tightenings(Some(&constraint)),
            vec![
                (String::new(), Tightening::Entries(vec!["intro"])),
                ("*.body".to_owned(), Tightening::Names("^[a-z]{2}$")),
                ("*.body.*".to_owned(), Tightening::Matches("[^ ]")),
                ("*.title".to_owned(), Tightening::Entries(vec!["en"])),
            ],
            "a struct's own `required` names fields, not entries"
        );
    }

    #[test]
    fn only_a_maps_single_pattern_is_an_entry_name_pattern() {
        let map = serde_json::json!({"type": "object", "propertyNames": {"pattern": "^[a-z]+$"}});
        assert_eq!(super::entry_name_pattern(Some(&map)), Some("^[a-z]+$"));
        // A struct's names are its fields, and a schema beyond a lone pattern is not the refinement.
        let fields = serde_json::json!({"type": "object", "properties": {}, "propertyNames": {"pattern": "x"}});
        assert_eq!(super::entry_name_pattern(Some(&fields)), None);
        let wider = serde_json::json!({"type": "object", "propertyNames": {"pattern": "x", "maxLength": 3}});
        assert_eq!(super::entry_name_pattern(Some(&wider)), None);
        assert_eq!(super::entry_name_pattern(None), None);
    }

    #[test]
    fn a_document_written_before_producer_existed_is_still_read() {
        // Legal `terrace_contract: 1` bytes: the field was added within the envelope, so refusing
        // one would be a reader that is as strict as a writer.
        let document = r#"{
            "terrace_contract": 1,
            "app": {"name": "x"},
            "schema": {
                "schema_version": 2,
                "dialect": {"prefix": "X_", "nesting_separator": "__",
                            "indirection_suffix": "_FILE"},
                "loader": [], "keys": []
            },
            "json_schema": {},
            "external": {"env": [], "ignore": [], "unknown": "reject"}
        }"#;
        let contract = Contract::from_json(document).expect("a document without a producer reads");
        assert!(contract.producer.is_none());
    }

    #[test]
    fn an_unreadable_envelope_version_names_itself() {
        let error = Contract::from_json(r#"{"terrace_contract": 99}"#)
            .expect_err("envelope 99 is not readable");
        assert!(error.to_string().contains("99"), "{error}");
    }

    #[test]
    fn a_vocabulary_this_build_does_not_know_is_not_a_parse_error() {
        // Every closed vocabulary carries an `other` arm, because a document from a later envelope
        // that this build *can* read must not fail on one unknown enum value in one key.
        assert_eq!(
            serde_json::from_str::<TextForm>(r#""duration""#).expect("an unknown form reads"),
            TextForm::Unknown
        );
        assert_eq!(
            serde_json::from_str::<Unreachable>(r#""shadowed""#).expect("an unknown reason reads"),
            Unreachable::Other
        );
        assert_eq!(
            serde_json::from_str::<LoaderRole>(r#""profile""#).expect("an unknown role reads"),
            LoaderRole::Other
        );
        assert_eq!(
            serde_json::from_str::<Unknown>(r#""audit""#).expect("an unknown policy reads"),
            Unknown::Other
        );
    }

    #[test]
    fn a_field_this_build_has_not_learned_is_kept_out_of_the_way() {
        let document = r#"{
            "terrace_contract": 1,
            "producer": {"name": "x", "version": "1", "loader": "figment"},
            "app": {"name": "x"},
            "schema": {
                "schema_version": 2,
                "dialect": {"prefix": "X_", "nesting_separator": "__",
                            "indirection_suffix": "_FILE"},
                "loader": [], "keys": []
            },
            "json_schema": {},
            "external": {"env": [], "ignore": [], "unknown": "reject"},
            "something_later": {"added": true}
        }"#;
        let contract = Contract::from_json(document).expect("an unknown field is not a refusal");
        assert_eq!(
            contract.producer.map(|producer| producer.loader).as_deref(),
            Some("figment")
        );
    }
}
