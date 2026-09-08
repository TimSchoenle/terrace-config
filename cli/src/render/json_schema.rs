//! The JSON Schema rendering: the document an editor validates a `config.toml` against.
//!
//! Nested keys become nested `properties` objects — `github.token` is two levels deep, exactly as
//! it is in the file — and a key with no default lands in its object's `required` list, which makes
//! the table holding it required in turn.
//!
//! **Nothing here derives a constraint.** [`Key::constraint`](crate::Key::constraint) arrived in
//! the document already composed, by a producer that had the type in front of it, and re-deriving
//! it from [`Key::ty`](crate::Key::ty) would be exactly the mistake `FORMAT.md` warns against: `ty`
//! is token text in the producer's language, and a `match` arm spelling `u64` is a renderer that
//! silently stops working on a document from a Java build. What this module does is *place* the
//! constraint the producer published, at the position in the document where the key lives.

use serde_json::{Map, Value as Json, json};

use super::tree::{self, Node};
use crate::document::{Key, Schema};

/// The 2020-12 meta-schema, which is what an editor assumes.
pub const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";
/// Draft 07, which is the dialect Helm validates `values.schema.json` against.
pub const DRAFT_07: &str = "http://json-schema.org/draft-07/schema#";

/// How much of a key's doc comment a rendering carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Docs {
    /// The whole comment. An editor renders it in a hover, and a hover is what paragraphs are for.
    #[default]
    Full,
    /// The first paragraph only.
    Summary,
    /// None of it.
    None,
}

impl Docs {
    fn of(self, docs: &str) -> Option<String> {
        let text = match self {
            Self::Full => docs.to_owned(),
            Self::Summary => super::markdown::summary(docs),
            Self::None => String::new(),
        };
        if text.is_empty() { None } else { Some(text) }
    }
}

/// How the JSON Schema rendering is shaped.
#[derive(Debug, Clone)]
pub struct JsonSchema {
    /// Which meta-schema the document declares.
    pub meta_schema: String,
    /// The document's own `$id`, if it has one.
    pub id: Option<String>,
    /// The document's `title`.
    pub title: Option<String>,
    /// How much of each key's comment becomes a `description`.
    pub docs: Docs,
    /// Whether a key's default is published as an annotation.
    pub defaults: bool,
    /// Whether an undeclared key is an error.
    pub closed: bool,
    /// Whether a key that must be supplied must be supplied *by this document*.
    ///
    /// The two meanings of "required" are not the same and this is where they part. A JSON Schema
    /// `required` says **this document must carry the property**;
    /// [`Key::required`](crate::Key::required) says **some layer must supply the key**, and the
    /// loader takes the environment or a mounted file just as readily.
    ///
    /// On for an editor validating a hand-written file, where the document is the only layer there
    /// is. Off for a consumer checking what a chart rendered, which checks `required` per key
    /// across every layer it can see instead — because a chart supplying a required *secret* from
    /// a mount, the only way to supply a secret, renders a document this refuses and a deployment
    /// that starts.
    pub require_present: bool,
}

impl Default for JsonSchema {
    fn default() -> Self {
        Self {
            meta_schema: DRAFT_2020_12.to_owned(),
            id: None,
            title: None,
            docs: Docs::Full,
            defaults: true,
            closed: true,
            require_present: true,
        }
    }
}

/// The schema as a JSON Schema document.
pub fn document(schema: &Schema, options: &JsonSchema) -> Map<String, Json> {
    // A reserved key is read from the environment before the layers exist, so no file supplies it
    // and a document describing one would be describing a key it cannot carry.
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

        // Every alias, as its own property. An alias is a spelling the loader accepts, so a closed
        // schema that left it out would underline a key that loads perfectly well.
        for alias in &key.aliases {
            let mut spelling = schema.clone();
            spelling.insert(
                "description".to_owned(),
                json!(format!("Another spelling of `{}`.", key.path)),
            );
            properties.insert(tree::name(alias).to_owned(), Json::Object(spelling));
        }

        properties.insert(name.to_owned(), Json::Object(schema));

        if key.required && options.require_present {
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
    // same name in one parent — resolves to the table. That is the half a reader can act on.
    for child in &node.children {
        properties.insert(
            child.segment.to_owned(),
            Json::Object(object(child, options)),
        );
        // A table is required because something inside it is, so it follows the same switch: with
        // `require_present` off there is nothing inside making it mandatory *here*.
        if child.required() && options.require_present {
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

    // Read off the key rather than re-derived from its type name. See this module's own note: the
    // producer composed this with the type in front of it, in a language this crate does not know.
    if let Some(Json::Object(constraint)) = &key.constraint {
        let mut constraint = constraint.clone();
        if options.closed {
            close(&mut constraint);
        }
        schema.extend(constraint);
    }

    if key.secret {
        // The keyword for a value that is written but never read back, which is as close as JSON
        // Schema comes to saying "credential" — and a hint an editor renders.
        schema.insert("writeOnly".to_owned(), json!(true));
    }

    if options.defaults
        && !key.secret
        && let Some(default) = &key.default_value
    {
        schema.insert("default".to_owned(), default.clone());
    }

    schema
}

/// Close every level a constraint describes, however deep.
fn close(schema: &mut Map<String, Json>) {
    if let Some(Json::Object(items)) = schema.get_mut("items") {
        close(items);
    }
    if let Some(Json::Object(values)) = schema.get_mut("additionalProperties") {
        close(values);
        return;
    }
    let Some(Json::Object(properties)) = schema.get_mut("properties") else {
        return;
    };
    for property in properties.values_mut() {
        if let Json::Object(property) = property {
            close(property);
        }
    }
    schema.insert("additionalProperties".to_owned(), json!(false));
}

/// The `description` a key carries: its comment, and what its default means.
fn description(key: &Key, options: &JsonSchema) -> Option<String> {
    let docs = options.docs.of(&key.docs);
    let note = key.note.as_ref().map(|note| format!("Default: {note}."));
    match (docs, note) {
        (Some(docs), Some(note)) => Some(format!("{docs}\n\n{note}")),
        (docs, note) => docs.or(note),
    }
}
