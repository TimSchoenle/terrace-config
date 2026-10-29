//! Describing a JSON Schema constraint in one line of plain words.

use serde_json::{Map, Value as Json};

/// Constraint keywords rendered as prose, in the order they read best.
///
/// Anything outside this list is printed as `keyword=<json>` rather than dropped: under-reporting a
/// constraint is the failure this whole pipeline exists to remove, and it would be no better coming
/// from the explainer.
pub(super) const CONSTRAINT_ORDER: [&str; 19] = [
    "type",
    "const",
    "enum",
    "pattern",
    "format",
    "minimum",
    "exclusiveMinimum",
    "maximum",
    "exclusiveMaximum",
    "multipleOf",
    "minLength",
    "maxLength",
    "minItems",
    "maxItems",
    "uniqueItems",
    "required",
    "properties",
    "additionalProperties",
    "items",
];

/// Keywords folded into another keyword's phrase, or said elsewhere in the entry.
///
/// Listed so the catch-all can tell "already reported" from "unrecognised".
pub(super) const CONSTRAINT_FOLDED: [&str; 13] = [
    "allOf",
    "propertyNames",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "maximum",
    "maxLength",
    "maxItems",
    "required",
    "description",
    "title",
    "default",
    "examples",
    "$comment",
];

/// A JSON Schema constraint object, as a phrase.
///
/// Bounds are folded into one range because that is how they are read (`0 to 65535`, not
/// `integer, minimum 0, maximum 65535`), and an unrecognised keyword is printed rather than
/// dropped, so a vocabulary this renderer does not know still reaches the reader.
///
/// Recurses, as of `schema_version: 2`. A container-typed key carries what one element holds under
/// `items` or `additionalProperties`, and printing that as raw JSON turns the one line an operator
/// reads about a setting into a wall of braces. `array of string one of "GET" | "POST"` is the same
/// fact.
///
/// A struct element is named by its fields rather than described in full: a route configuration has
/// twenty of them, each with a type and a bound of its own, and a table cell is not where that
/// belongs. The JSON form emits the constraint verbatim for a reader that wants all of it.
#[must_use]
pub fn describe_constraint(constraint: Option<&Json>) -> String {
    let Some(schema) = constraint
        .and_then(Json::as_object)
        .filter(|held| !held.is_empty())
    else {
        return String::new();
    };

    let mut parts: Vec<String> = Vec::new();
    for keyword in CONSTRAINT_ORDER {
        let Some(value) = schema.get(keyword) else {
            continue;
        };
        match keyword {
            "type" => parts.push(match value {
                Json::String(name) => name.clone(),
                Json::Array(names) => names
                    .iter()
                    .map(|name| name.as_str().unwrap_or_default().to_owned())
                    .collect::<Vec<_>>()
                    .join(" or "),
                other => other.to_string(),
            }),
            "const" => parts.push(format!("exactly {value}")),
            "enum" => parts.push(format!(
                "one of {}",
                value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" | ")
            )),
            "pattern" => parts.push(format!("matching {}", plain(value))),
            "format" => parts.push(format!("format {}", plain(value))),
            "multipleOf" => parts.push(format!("a multiple of {value}")),
            "minimum" | "exclusiveMinimum" => parts.push(range(schema)),
            "maximum" | "exclusiveMaximum" => {
                if !schema.contains_key("minimum") && !schema.contains_key("exclusiveMinimum") {
                    parts.push(range(schema));
                }
            }
            "minLength" => parts.push(length(schema)),
            "maxLength" => {
                if !schema.contains_key("minLength") {
                    parts.push(length(schema));
                }
            }
            "minItems" => parts.push(count(schema)),
            "maxItems" => {
                if !schema.contains_key("minItems") {
                    parts.push(count(schema));
                }
            }
            "uniqueItems" => {
                if value.as_bool() == Some(true) {
                    parts.push("distinct".to_owned());
                }
            }
            "properties" => parts.push(fields(value, schema.get("required"))),
            "items" | "additionalProperties"
                if value.as_object().is_some_and(|held| !held.is_empty()) =>
            {
                let inner = describe_constraint(Some(value));
                if !inner.is_empty() {
                    // Parenthesised where the element has more than one thing to say about it, so
                    // `object, of (array, distinct)` cannot be read as a distinct object.
                    parts.push(if inner.contains(',') {
                        format!("of ({inner})")
                    } else {
                        format!("of {inner}")
                    });
                }
            }
            _ => {}
        }
    }

    said_refinements(schema, &mut parts);

    for (keyword, value) in schema {
        if !CONSTRAINT_ORDER.contains(&keyword.as_str())
            && !CONSTRAINT_FOLDED.contains(&keyword.as_str())
        {
            parts.push(format!("{keyword}={value}"));
        }
    }
    parts.join(", ")
}

/// What a producer's refinements added to a constraint, each in the words a reader acts on.
pub(super) fn said_refinements(schema: &Map<String, Json>, parts: &mut Vec<String>) {
    // A map's required entries: `required` beside no `properties`, which a producer's refinement
    // puts there. A struct's `required` stays folded into its fields line, which marks each one.
    let entries = crate::document::required_entries_of(schema);
    if !entries.is_empty() {
        parts.push(format!("must contain: {}", entries.join(", ")));
    }
    // A struct's conditions, by the sentence each carries. An `allOf` member without one is not a
    // condition this format defines, and is printed as it stands.
    if let Some(Json::Array(members)) = schema.get("allOf") {
        let mut unsaid = Vec::new();
        for member in members {
            match member.get("description").and_then(Json::as_str) {
                Some(description) => parts.push(format!("holds: {description}")),
                None => unsaid.push(member.clone()),
            }
        }
        if !unsaid.is_empty() {
            parts.push(format!("allOf={}", Json::Array(unsaid)));
        }
    }
    // A map's entry-name pattern, the other refinement. Anything else under `propertyNames` is not
    // one this format defines, and is printed as it stands rather than paraphrased.
    if let Some(names) = schema.get("propertyNames") {
        parts.push(match crate::document::entry_name_pattern_of(schema) {
            Some(pattern) => format!("entry names matching {pattern}"),
            None => format!("propertyNames={names}"),
        });
    }
}

/// One scalar without the quotes JSON would put round it.
pub(super) fn plain(value: &Json) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// The numeric bounds, folded into one phrase.
pub(super) fn range(schema: &Map<String, Json>) -> String {
    let held = |name: &str| schema.get(name).map(ToString::to_string);
    let (low, low_open) = (held("minimum"), held("exclusiveMinimum"));
    let (high, high_open) = (held("maximum"), held("exclusiveMaximum"));
    let lower = low_open
        .as_ref()
        .map(|bound| format!("above {bound}"))
        .or(low);
    let upper = high_open
        .as_ref()
        .map(|bound| format!("below {bound}"))
        .or(high);

    match (lower, upper) {
        (Some(lower), Some(upper)) => {
            if low_open.is_none() && high_open.is_none() {
                format!("{lower} to {upper}")
            } else {
                format!("{lower}, {upper}")
            }
        }
        (Some(lower), None) => {
            if low_open.is_some() {
                lower
            } else {
                format!("at least {lower}")
            }
        }
        (None, Some(upper)) => {
            if high_open.is_some() {
                upper
            } else {
                format!("at most {upper}")
            }
        }
        (None, None) => String::new(),
    }
}

/// `minItems` and `maxItems`, folded the way [`length`] folds their string equivalents.
pub(super) fn count(schema: &Map<String, Json>) -> String {
    match (schema.get("minItems"), schema.get("maxItems")) {
        (Some(low), Some(high)) => format!("{low} to {high} items"),
        (Some(low), None) => format!("at least {low} item(s)"),
        (None, Some(high)) => format!("at most {high} item(s)"),
        (None, None) => String::new(),
    }
}

/// A struct element's fields, in the order the producer declared them, required ones starred.
///
/// Declaration order rather than alphabetical: that is the order the struct is written in, and the
/// order somebody comparing this against the source will read it in.
pub(super) fn fields(properties: &Json, required: Option<&Json>) -> String {
    let required: Vec<&str> = required
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
        .collect();
    let names: Vec<String> = properties
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| {
            if required.contains(&name.as_str()) {
                format!("{name}*")
            } else {
                name.clone()
            }
        })
        .collect();
    if names.is_empty() {
        "no fields".to_owned()
    } else {
        format!("fields {}", names.join(", "))
    }
}

/// The string-length bounds, folded into one phrase.
pub(super) fn length(schema: &Map<String, Json>) -> String {
    match (schema.get("minLength"), schema.get("maxLength")) {
        (Some(low), Some(high)) => format!("{low} to {high} characters"),
        (Some(low), None) => format!("at least {low} characters"),
        (None, Some(high)) => format!("at most {high} characters"),
        (None, None) => String::new(),
    }
}
