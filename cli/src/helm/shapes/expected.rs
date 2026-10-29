//! Building the schema a value's block is expected to carry from the contract key's constraint.

use serde_json::{Map, Value as Json};

use super::ORDER;

/// Whether a constraint says anything a schema could hold a value to.
///
/// A key the producer publishes no constraint for would leave [`expected`] with a block naming all
/// six JSON types, which is a property a schema generator emits and nothing rejects. As an opt-in
/// that was harmless; as the default it is a downgrade, so the generator refuses the key instead and
/// asks for the transcription to be declared as one.
pub fn describing(constraint: Option<&Json>) -> bool {
    constraint.and_then(Json::as_object).is_some_and(|held| {
        ["type", "enum", "const"]
            .iter()
            .any(|name| held.contains_key(*name))
    })
}

/// What one element of a container-typed key holds, or [`None`] when the contract does not say.
///
/// `items` for a sequence and `additionalProperties` for a map, which is the whole of the
/// `schema_version: 2` addition at one level. `true` and `false` are the open/closed flag rather
/// than a schema, and are not an element description.
pub fn element_schema(constraint: Option<&Json>) -> Option<&Json> {
    let held = constraint?.as_object()?;
    ["items", "additionalProperties"]
        .into_iter()
        .find_map(|keyword| {
            held.get(keyword)
                .filter(|element| element.as_object().is_some_and(|held| !held.is_empty()))
        })
}

/// Whether the contract says what one element of a container-typed key holds.
pub fn describes_element(constraint: Option<&Json>) -> bool {
    element_schema(constraint).is_some()
}

/// The `@schema` mapping one contract key's constraint calls for.
///
/// Two shapes are not a straight copy, and both were measured against the schema generator rather
/// than reasoned about:
///
/// **An `enum` is the whole block.** The generator refuses one carrying both `enum` and `type`, and
/// exits fatally, leaving every chart in the repository without a generated schema. So a key whose
/// constraint names an `enum` emits its members alone, with `null` joining them where the value is
/// optional. Measured again for `schema_version: 2`: the refusal applies to the *top level only*. A
/// nested subschema carrying both passes through, and that is what makes a generated element schema
/// possible at all: every enum inside a struct is an `enum` beside a `type`.
///
/// **A `structured` key whose element is undescribed is opened rather than described.** Such a
/// constraint says the value is a table and nothing about what is in it, so the block accepts any
/// table. A key whose element *is* described takes the description instead, which is the whole of
/// what `schema_version: 2` buys.
///
/// **A `structured` key whose constraint names an array is neither.** It carries its own `items`,
/// and the value beside it is a list, so describing it as an object makes a chart reject its own
/// defaults the moment its schema is generated.
pub fn expected(constraint: Option<&Json>, optional: bool, structured: bool) -> Json {
    let empty = Map::new();
    let held = constraint.and_then(Json::as_object).unwrap_or(&empty);

    if let Some(Json::Array(members)) = held.get("enum") {
        let mut members = members.clone();
        if optional && !members.contains(&Json::Null) {
            members.push(Json::Null);
        }
        let mut schema = Map::new();
        schema.insert("enum".to_owned(), Json::Array(members));
        return Json::Object(schema);
    }

    let declared = held.get("type");
    let mut types: Vec<String> = match declared {
        Some(Json::Array(names)) => names
            .iter()
            .filter(|name| !name.is_null())
            .map(scalar_name)
            .collect(),
        Some(Json::Null) | None => Vec::new(),
        Some(single) => vec![scalar_name(single)],
    };

    let mut schema = Map::new();
    let element = element_schema(constraint);

    if structured
        && (types.is_empty() || types.iter().any(|name| name == "object"))
        && !held.contains_key("items")
    {
        types = vec!["object".to_owned()];
        // The producer's own fields, where it published any. A table key is normally split into one
        // contract key per field, so this is the hand-written case rather than the derive's, and
        // dropping the fields there would describe a documented struct as an open table. A map's
        // entry-name pattern travels with them, for the reason its required entries do.
        for keyword in ["required", "properties", "propertyNames"] {
            if let Some(value) = held.get(keyword) {
                schema.insert(keyword.to_owned(), copy_keyword(keyword, value));
            }
        }
        // `additionalProperties: true` is the open flag, and an element schema replaces it: a map
        // whose values are all one shape is not open, it is uniform. Written even beside enumerated
        // properties, because a generator injects `additionalProperties: false` into a top-level
        // block that enumerates them and says nothing, and the contract's silence here means open,
        // not closed: a deserialiser accepts a field nobody declared unless the type says
        // otherwise.
        schema.insert(
            "additionalProperties".to_owned(),
            element.map_or(Json::Bool(true), copy),
        );
    } else {
        for keyword in ORDER {
            if keyword == "type" || keyword == "enum" {
                continue;
            }
            if let Some(value) = held.get(keyword) {
                schema.insert(keyword.to_owned(), copy_keyword(keyword, value));
            }
        }
    }

    if types.is_empty() {
        // The constraint names no type. Rather than guess one, it accepts every JSON type: the
        // contract is still the authority on the value, and the document gate holds the rendered
        // configuration against it either way.
        types = ["string", "integer", "boolean", "array", "object", "null"]
            .into_iter()
            .map(str::to_owned)
            .collect();
    }
    // An optional value is one the chart may legitimately leave unset, and a derived helper omits
    // it rather than writing it empty, so `null` has to be a value the schema accepts.
    if optional && !types.iter().any(|name| name == "null") {
        types.push("null".to_owned());
    }

    let mut out = Map::new();
    out.insert(
        "type".to_owned(),
        if types.len() == 1 {
            Json::String(types.remove(0))
        } else {
            Json::Array(types.into_iter().map(Json::String).collect())
        },
    );
    for (name, value) in schema {
        out.insert(name, value);
    }
    Json::Object(out)
}

/// One JSON scalar as the name a `type` keyword spells it with.
pub(super) fn scalar_name(value: &Json) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// One subschema, with every annotation dropped at every level.
///
/// `format` is kept: it is the one annotation a validator acts on. Everything else is copied rather
/// than translated: a constraint is JSON Schema and so is an `@schema` block, so a `minimum` means
/// the same thing on both sides.
pub(super) fn copy(value: &Json) -> Json {
    match value {
        Json::Object(fields) => Json::Object(
            fields
                .iter()
                .filter(|(name, _)| !is_annotation(name))
                .map(|(name, inner)| (name.clone(), copy_keyword(name, inner)))
                .collect(),
        ),
        Json::Array(items) => Json::Array(items.iter().map(copy).collect()),
        other => other.clone(),
    }
}

/// One keyword's value, copied as the thing that keyword holds.
///
/// `properties` is the only container keyword whose value is a mapping of *names* to schemas;
/// `items` and `additionalProperties` each hold a schema outright. So it is the one place a key
/// must not be read as a keyword, and the one place [`copy`] may not strip by name: a struct with a
/// field called `description` is an ordinary struct.
pub(super) fn copy_keyword(keyword: &str, value: &Json) -> Json {
    if keyword == "properties"
        && let Json::Object(fields) = value
    {
        return Json::Object(
            fields
                .iter()
                .map(|(name, subschema)| (name.clone(), copy(subschema)))
                .collect(),
        );
    }
    copy(value)
}

/// Whether a keyword says something to a reader and asserts nothing about a value.
pub(super) fn is_annotation(name: &str) -> bool {
    matches!(
        name,
        "description" | "title" | "default" | "examples" | "$comment"
    )
}
