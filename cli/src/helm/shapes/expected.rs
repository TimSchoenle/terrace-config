//! Building the schema a value's block is expected to carry from the contract key's constraint.

use serde_json::{Map, Value as Json, json};

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
/// Four shapes are not a straight copy, and each was measured against the schema generator the
/// chart repositories pin (helm-schema 0.18.1) rather than reasoned about:
///
/// **A value list is never beside `type`.** The generator refuses a subschema carrying `enum` or
/// `const` together with `type`, and exits fatally, leaving every chart in the repository without a
/// generated schema. It holds that rule wherever its validator walks, which is not everywhere; see
/// `settle` for the positions. So a key whose constraint names an `enum` or a `const` emits the
/// list alone, and every element schema copied below is settled the same way.
///
/// **An optional value list is wrapped rather than extended.** The generator reads `enum` as a list
/// of *strings*, so a `null` member does not survive into `values.schema.json`, and the value's own
/// default (`null`, which is what an unset optional value is written as) then fails every render.
/// So "the members, or unset" is spelt `anyOf: [{type: 'null'}, {enum: [...]}]`, which the
/// generator carries through intact. Writing `""` as the unset member instead was rejected: it is
/// a setting the contract does not accept, it is only harmless while the template happens to guard
/// with `with`, and it cannot be spelt at all for a list of numbers.
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

    if let Some(listed) = listing(held, optional) {
        return listed;
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
            if keyword == "type" {
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
    let mut out = Json::Object(out);
    settle(&mut out);
    out
}

/// The block for a constraint that lists its legal values, or [`None`] when it lists none.
///
/// `enum` wins over `const` when a constraint carries both, because it is what the rest of this
/// toolchain reads a choice from. `null` is never kept as a member: the generator cannot carry it
/// (see [`expected`]), so a list that includes it, or a value that is optional, accepts it through
/// a `type: 'null'` alternative instead.
fn listing(held: &Map<String, Json>, optional: bool) -> Option<Json> {
    let (keyword, listed): (&str, Vec<Json>) = match (held.get("enum"), held.get("const")) {
        (Some(Json::Array(members)), _) => ("enum", members.clone()),
        (_, Some(value)) => ("const", vec![value.clone()]),
        _ => return None,
    };
    let nullable = optional || listed.contains(&Json::Null);
    let members: Vec<Json> = listed.into_iter().filter(|held| !held.is_null()).collect();

    let unset = json!({"type": "null"});
    if members.is_empty() {
        // `enum: [null]` or `const: null`: the one legal value is the absence of one.
        return Some(unset);
    }
    let mut list = Map::new();
    list.insert(
        keyword.to_owned(),
        if keyword == "const" {
            members.into_iter().next().unwrap_or(Json::Null)
        } else {
            Json::Array(members)
        },
    );
    let list = Json::Object(list);
    Some(if nullable {
        json!({"anyOf": [unset, list]})
    } else {
        list
    })
}

/// The subschema keywords holding one schema that the generator validates, as it does the block.
const VALIDATED_ONE: [&str; 5] = ["items", "not", "if", "then", "else"];

/// The subschema keywords holding a list of schemas that the generator validates.
const VALIDATED_MANY: [&str; 3] = ["anyOf", "allOf", "oneOf"];

/// A block with every `type` the generator would refuse beside a value list removed.
///
/// helm-schema 0.18.1 checks each block with `Schema.Validate`, which refuses `enum` or `const`
/// beside `type` at the block itself and then recurses into exactly the positions in
/// [`VALIDATED_ONE`] and [`VALIDATED_MANY`], each held to the same rule, so `items.items` is
/// refused as surely as `items`. It does *not* recurse into `properties` or `additionalProperties`:
/// an `enum` beside a `type` on a struct field passes, at any depth below one of those. That is
/// what keeps every enumerated field of a generated element schema typed, and it is why this walks
/// the generator's positions rather than every subschema: settling a field the generator accepts
/// would rewrite blocks that are correct today, for nothing.
///
/// The value list wins because it is the stricter of the two, so dropping `type` loses no value the
/// contract refuses.
pub(super) fn settle(schema: &mut Json) {
    let Json::Object(fields) = schema else {
        return;
    };
    if fields.contains_key("type") && (fields.contains_key("enum") || fields.contains_key("const"))
    {
        fields.remove("type");
    }
    for keyword in VALIDATED_ONE {
        if let Some(inner) = fields.get_mut(keyword) {
            settle(inner);
        }
    }
    for keyword in VALIDATED_MANY {
        if let Some(Json::Array(members)) = fields.get_mut(keyword) {
            members.iter_mut().for_each(settle);
        }
    }
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
