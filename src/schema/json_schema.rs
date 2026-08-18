//! The JSON Schema rendering: what an editor validates the TOML file against.
//!
//! The other three renderings describe the configuration to a person. This one describes it to a
//! program, and the programs are worth naming because they decide what belongs in it:
//!
//! - **An editor.** Point a TOML language server at this file and every key completes, every
//!   `///` comment is the hover text, and `port = "8080"` is underlined in the editor rather
//!   than at the next deployment.
//! - **A Helm chart.** `values.schema.json` is validated by `helm install` before anything
//!   reaches a cluster, and a chart whose values *are* the service's configuration has been
//!   hand-writing this file until now.
//!
//! # What it does not claim
//!
//! A JSON Schema is a set of things a document must not be, so every keyword here has to be
//! certainly true — a schema that rejects a file the loader would have accepted is worse than one
//! that accepts a file the loader will reject, because the first stops a deployment that was
//! correct. Two consequences:
//!
//! - `type` comes from [`super::rust_type`], which produces nothing at all for a spelling it does
//!   not recognise. A key whose type is a domain newtype is left unconstrained.
//! - A [`reserved`](super::Key::reserved) key is left out entirely. The loader reads it from the
//!   environment and a file may not supply it, so a schema listing it would complete a key that
//!   does nothing — and with [`JsonSchema::closed`] on, a file that sets one is flagged, which is
//!   what the loader says about it too.
//! - Every [`alias`](super::Key::aliases) is a property of its own. A closed schema that listed
//!   only the canonical spelling would underline `user = "…"` in a file that loads, which is the
//!   rejection this whole section is about.

use serde_json::{Map, Value as Json, json};

use super::tree::{self, Node};
use super::{Docs, Error, Key, Schema, rust_type};

/// The meta-schema URI for JSON Schema 2020-12 — what a current editor implements.
pub const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// The meta-schema URI for JSON Schema draft-07 — what Helm validates `values.schema.json`
/// against.
///
/// Nothing but the URI changes. Every keyword this rendering emits — `type`, `enum`,
/// `properties`, `required`, `additionalProperties`, `items`, `uniqueItems`, the numeric bounds,
/// `not`, `const`, `default`, `description`, `writeOnly` — is in draft-07 with the same meaning
/// it has in 2020-12, so the choice is which dialect the consumer wants to be told it is reading.
pub const DRAFT_07: &str = "http://json-schema.org/draft-07/schema#";

/// How [`Schema::to_json_schema`] renders.
///
/// ```
/// # use terrace_config::Terrace;
/// # use terrace_config::schema::{Describe, JsonSchema, Leaf, Sink, DRAFT_07};
/// # struct Config;
/// # impl Describe for Config {
/// #     fn describe(sink: &mut Sink) {
/// #         sink.leaf(Leaf { name: "replicas", docs: "", ty: Some("u32"), values: None,
/// #             aliases: &[], note: None, required: false, secret: false });
/// #     }
/// # }
/// // A Helm chart's `values.schema.json`: draft-07, named, and open to the keys Helm itself adds.
/// let options = JsonSchema::new()
///     .meta_schema(DRAFT_07)
///     .title("myservice values")
///     .closed(false);
///
/// let rendered = Terrace::new("MYSERVICE_").schema::<Config>().to_json_schema_with(&options)?;
/// # Ok::<(), terrace_config::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct JsonSchema {
    /// The dialect the document declares, as a `$schema` URI.
    meta_schema: String,
    /// The document's `$id`, for a schema published at a URL.
    id: Option<String>,
    /// The document's `title`.
    title: Option<String>,
    /// How much of each key's `///` comment becomes its `description`.
    docs: Docs,
    /// Whether to carry each key's default as a `default` annotation.
    defaults: bool,
    /// Whether a key the configuration does not have is an error.
    closed: bool,
}

impl Default for JsonSchema {
    fn default() -> Self {
        Self {
            meta_schema: DRAFT_2020_12.to_owned(),
            id: None,
            title: None,
            // No page to fit inside and no cell to overflow: an editor renders the whole comment
            // in a hover, and the paragraphs below the summary are what a hover is for.
            docs: Docs::Full,
            defaults: true,
            closed: true,
        }
    }
}

impl JsonSchema {
    /// JSON Schema 2020-12, closed, with every comment and every default.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The dialect to declare. Defaults to [`DRAFT_2020_12`]; [`DRAFT_07`] is the other one that
    /// is known to be true of this output.
    #[must_use]
    pub fn meta_schema(mut self, uri: impl Into<String>) -> Self {
        self.meta_schema = uri.into();
        self
    }

    /// The document's `$id`. Omitted by default.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// The document's `title`. Omitted by default.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The document's `title`, unless one was already chosen.
    ///
    /// For a caller supplying a default the user is expected to override rather than one that
    /// overrides the user — which is [`Contract`](super::Contract), whose title falls back to the
    /// app's name and must not silently replace a title the generator asked for.
    pub(super) fn or_title(mut self, title: impl Into<String>) -> Self {
        self.title = self.title.or_else(|| Some(title.into()));
        self
    }

    /// How much of each key's `///` comment becomes its `description`. Defaults to [`Docs::Full`].
    #[must_use]
    pub fn docs(mut self, docs: Docs) -> Self {
        self.docs = docs;
        self
    }

    /// Whether each key carries its default as a `default` annotation. Defaults to `true`.
    ///
    /// An annotation, not a behaviour: no validator supplies it, and neither does Helm. It is
    /// there for the editor that offers it as the completion. A [`secret`](Key::secret) key never
    /// carries one whatever this says — there is nothing to carry, because
    /// [`Schema::with_defaults_from`] keeps no value for one.
    #[must_use]
    pub fn defaults(mut self, defaults: bool) -> Self {
        self.defaults = defaults;
        self
    }

    /// Whether a key the configuration does not have is an error. Defaults to `true`.
    ///
    /// On, because the loader's own posture is that a key nobody reads is a mistake rather than a
    /// courtesy — `ShadowPolicy` exists to make one loud — and a misspelled key in a config file
    /// is silently ignored by `serde` unless something says otherwise. This is that something.
    ///
    /// Off for a schema that describes *part* of a document: a Helm chart's values carry keys
    /// belonging to the chart rather than to the service, and `additionalProperties: false` at
    /// the root would reject every one of them.
    #[must_use]
    pub fn closed(mut self, closed: bool) -> Self {
        self.closed = closed;
        self
    }
}

impl Schema {
    /// The schema as a JSON Schema document, for an editor or a Helm chart to validate against.
    ///
    /// Nested keys become nested `properties` objects — `csp.cloudflare.turnstile` is three
    /// levels deep, exactly as it is in the file — and a key with no default lands in its
    /// object's `required` list, which makes the table holding it required in turn.
    ///
    /// Use [`Self::to_json_schema_with`] to choose the dialect, the title, or how strict it is.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] if serialisation fails, which for this type means the JSON
    /// writer failed rather than the data being unrepresentable.
    pub fn to_json_schema(&self) -> Result<String, Error> {
        self.to_json_schema_with(&JsonSchema::default())
    }

    /// The same document, with a chosen dialect and set of annotations.
    ///
    /// See [`JsonSchema`].
    ///
    /// # Errors
    /// As [`Self::to_json_schema`].
    pub fn to_json_schema_with(&self, options: &JsonSchema) -> Result<String, Error> {
        serde_json::to_string_pretty(&Json::Object(document(self, options))).map_err(|e| {
            Error::Invalid(format!("the JSON Schema could not be written as JSON: {e}"))
        })
    }
}

/// The document itself, before anything decides how to write it out.
///
/// Split from [`Schema::to_json_schema_with`] because a JSON Schema has two destinations and only
/// one of them is a file: [`Contract`](super::Contract) carries this one *inside* a larger
/// document, and serialising it to a string only to parse it back would be both slower and a
/// second place for the two to disagree about what was rendered.
pub(super) fn document(schema: &Schema, options: &JsonSchema) -> Map<String, Json> {
    let reachable = schema.keys.iter().filter(|key| !key.reserved);
    let mut document = object(&Node::of(reachable), options);

    document.insert("$schema".to_owned(), json!(options.meta_schema));
    if let Some(id) = &options.id {
        document.insert("$id".to_owned(), json!(id));
    }
    if let Some(title) = &options.title {
        document.insert("title".to_owned(), json!(title));
    }

    document
}

/// One level of the configuration as a JSON Schema object.
fn object(node: &Node<'_>, options: &JsonSchema) -> Map<String, Json> {
    let mut properties = Map::new();
    let mut required = Vec::new();
    // A required key with aliases is not one property that must be present but a *choice* of
    // properties, one of which must be. `required` cannot say that and `anyOf` can, so those keys
    // collect here and go in under one `allOf` — a second `anyOf` on the same object would
    // overwrite the first.
    let mut either = Vec::new();

    for key in &node.keys {
        let name = tree::name(&key.path);
        let schema = leaf(key, options);

        // Every alias, as its own property. An alias is a spelling the loader accepts, so a
        // closed schema that left it out would underline a key that loads perfectly well — the
        // same bug the Markdown table's `Aliases` column exists to keep out of the reference.
        for alias in &key.aliases {
            let mut spelling = schema.clone();
            spelling.insert(
                "description".to_owned(),
                json!(format!("Another spelling of `{}`.", key.path)),
            );
            properties.insert(tree::name(alias).to_owned(), Json::Object(spelling));
        }

        properties.insert(name.to_owned(), Json::Object(schema));

        if key.required {
            if key.aliases.is_empty() {
                required.push(json!(name));
            } else {
                let spellings: Vec<Json> = std::iter::once(name)
                    .chain(key.aliases.iter().map(|alias| tree::name(alias)))
                    .map(|spelling| json!({ "required": [spelling] }))
                    .collect();
                either.push(json!({ "anyOf": spellings }));
            }
        }
    }
    // After the leaves, so that the one shape neither format can carry — a key and a table of the
    // same name in one parent — resolves to the table. That is the half a reader can act on, and
    // it is what the TOML rendering comments the key out in favour of.
    for child in &node.children {
        properties.insert(
            child.segment.to_owned(),
            Json::Object(object(child, options)),
        );
        if child.required() {
            required.push(json!(child.segment));
        }
    }

    let mut schema = Map::new();
    schema.insert("type".to_owned(), json!("object"));
    schema.insert("properties".to_owned(), Json::Object(properties));
    if !required.is_empty() {
        schema.insert("required".to_owned(), Json::Array(required));
    }
    if !either.is_empty() {
        schema.insert("allOf".to_owned(), Json::Array(either));
    }
    if options.closed {
        schema.insert("additionalProperties".to_owned(), json!(false));
    }
    schema
}

/// One key as a JSON Schema subschema.
fn leaf(key: &Key, options: &JsonSchema) -> Map<String, Json> {
    let mut schema = Map::new();

    if let Some(description) = description(key, options) {
        schema.insert("description".to_owned(), json!(description));
    }

    if let Some(constraint) = constraint(key.ty.as_deref(), &key.values) {
        schema.extend(constraint);
    }

    if key.secret {
        // The keyword for a value that is written but never read back, which is as close as JSON
        // Schema comes to saying "credential" — and a hint an editor renders.
        schema.insert("writeOnly".to_owned(), json!(true));
    }

    if options.defaults
        && !key.secret
        && let Some(value) = &key.default_value
        // A default that JSON cannot hold — an infinity, an integer past `f64`'s exact range —
        // is left out. It is an annotation, so losing one costs a completion rather than a
        // validation, and writing it wrong would cost the validation.
        && let Ok(default) = serde_json::to_value(value)
    {
        schema.insert("default".to_owned(), default);
    }

    schema
}

/// What a value of this type must be, as JSON Schema keywords — `type`, `enum`, the numeric
/// bounds, `items`, `uniqueItems`.
///
/// The whole of what a *type* can say and nothing about the key that has it, which is what makes
/// it reusable: [`leaf`] adds the description and the default for a key in a rendered document,
/// and [`Key::constraint`] carries it flat for a consumer checking the *environment variable* that
/// supplies the same key. Every value in an environment is a string, so that consumer has nothing
/// but this to check against — and without it, every consumer in every language reimplements a
/// vocabulary of Rust type names, with `PathBuf` as the trap: it is a string and nothing in the
/// name says so.
///
/// [`None`] means unconstrained: a type [`rust_type::interpret`] does not recognise, and no fixed
/// set of values. A domain newtype lands here, and a validator can say nothing about it beyond
/// that the key exists.
pub(super) fn constraint(ty: Option<&str>, values: &[String]) -> Option<Map<String, Json>> {
    if values.is_empty() {
        return ty.and_then(rust_type::interpret);
    }

    // A fixed set of values is stronger than any type could be, and it is always a set of strings:
    // `Values::VARIANTS` holds the spellings `serde` accepts for unit variants.
    let mut schema = Map::new();
    schema.insert("type".to_owned(), json!("string"));
    schema.insert("enum".to_owned(), json!(values));
    Some(schema)
}

/// The `description` for a key: its comment, and what its default means.
///
/// The [`note`](Key::note) is here rather than left to [`Schema::to_json`] because this is the
/// text an operator reads in a hover at the moment they are deciding whether to set the key, and
/// "permanent" is the sentence that decides it.
fn description(key: &Key, options: &JsonSchema) -> Option<String> {
    let docs = options.docs.of(&key.docs);
    let note = key.note.as_ref().map(|note| format!("Default: {note}."));
    match (docs, note) {
        (Some(docs), Some(note)) => Some(format!("{docs}\n\n{note}")),
        (docs, note) => docs.or(note),
    }
}
