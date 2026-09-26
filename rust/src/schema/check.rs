//! Whether a value satisfies a published constraint, as far as this crate can prove it.
//!
//! [`ContractBuilder::build`](super::ContractBuilder::build) refuses a key whose default fails its
//! own [`Key::constraint`](super::Key::constraint), and [`Schema::refine`](super::Schema::refine)
//! is the API that made that possible to write: a refinement tightens a constraint after a default
//! was already observed against the looser one. Refusing needs an evaluator, and this crate links
//! no JSON Schema engine — it is a library a service links, and the engine is a dev-dependency on
//! purpose.
//!
//! So this is a small one, over exactly the vocabulary a producer here emits into a constraint, and
//! it is written to the same rule as everything it evaluates: **a refusal must be certain.** Every
//! answer is one of three, and only [`Verdict::Fails`] refuses. A keyword outside the vocabulary
//! makes a schema [`Verdict::Undecided`] rather than satisfied, which matters in exactly one place —
//! under `not`, where an undecided subschema must not be read as one that holds.

use serde_json::{Map, Number, Value as Json};

/// How deep the evaluator follows nested schemas before it stops deciding.
///
/// [`super::MAX_DEPTH`]'s reason: a constraint is a tree the caller can build by hand, and a stack
/// overflow in a build step is worse than an answer of "not decided".
const MAX_DEPTH: usize = super::MAX_DEPTH;

/// What evaluating one value against one schema established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Verdict {
    /// Every keyword was evaluated and none failed.
    Holds,
    /// A keyword failed, in a sentence naming where and how.
    Fails(String),
    /// Nothing failed, and something could not be evaluated — a keyword outside the vocabulary,
    /// or a tree deeper than [`MAX_DEPTH`].
    Undecided,
}

impl Verdict {
    /// The two verdicts combined as `allOf` combines them: the first failure wins, and anything
    /// undecided keeps the whole from holding.
    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Fails(reason), _) | (_, Self::Fails(reason)) => Self::Fails(reason),
            (Self::Undecided, _) | (_, Self::Undecided) => Self::Undecided,
            (Self::Holds, Self::Holds) => Self::Holds,
        }
    }
}

/// Keywords that say something to a reader and assert nothing about a value.
const ANNOTATIONS: &[&str] = &["description", "title", "default", "examples", "$comment"];

/// `value` against `constraint`.
///
/// A constraint that is not an object is [`Verdict::Undecided`]: `true` and `false` are legal JSON
/// Schemas, but no producer here emits either as a key's constraint, and guessing at one is not
/// what a refusal can rest on.
pub(super) fn verdict(constraint: &Json, value: &Json) -> Verdict {
    evaluate(constraint, value, "", 0)
}

fn evaluate(schema: &Json, value: &Json, at: &str, depth: usize) -> Verdict {
    if depth > MAX_DEPTH {
        return Verdict::Undecided;
    }
    let Some(schema) = schema.as_object() else {
        return Verdict::Undecided;
    };

    let mut result = Verdict::Holds;
    for (keyword, argument) in schema {
        let found = match keyword.as_str() {
            "type" => of_type(argument, value, at),
            "enum" => one_of(argument, value, at),
            "const" => equal(argument, value, at),
            "not" => negated(argument, value, at, depth),
            "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" => {
                bound(keyword, argument, value, at)
            }
            "minLength" | "maxLength" => length(keyword, argument, value, at),
            "pattern" => pattern(argument, value, at),
            "minItems" | "maxItems" => count(keyword, argument, value, at),
            "uniqueItems" => unique(argument, value, at),
            "items" => items(argument, value, at, depth),
            "required" => required(argument, value, at),
            "properties" => properties(argument, value, at, depth),
            "additionalProperties" => additional(schema, argument, value, at, depth),
            "propertyNames" => names(argument, value, at, depth),
            "allOf" => all_of(argument, value, at, depth),
            "anyOf" => any_of(argument, value, at, depth),
            annotation if ANNOTATIONS.contains(&annotation) => Verdict::Holds,
            _ => Verdict::Undecided,
        };
        result = result.and(found);
        if matches!(result, Verdict::Fails(_)) {
            return result;
        }
    }
    result
}

/// A failure at `at`, or at the value itself when `at` is empty.
fn fails(at: &str, message: &str) -> Verdict {
    if at.is_empty() {
        Verdict::Fails(message.to_owned())
    } else {
        Verdict::Fails(format!("`{at}` {message}"))
    }
}

fn of_type(argument: &Json, value: &Json, at: &str) -> Verdict {
    let names: Vec<&str> = match argument {
        Json::String(name) => vec![name.as_str()],
        Json::Array(names) => names.iter().filter_map(Json::as_str).collect(),
        _ => return Verdict::Undecided,
    };
    if names.iter().any(|name| is_type(value, name)) {
        Verdict::Holds
    } else {
        fails(at, &format!("is {value}, which is not of type {argument}"))
    }
}

/// Whether `value` is an instance of the JSON Schema type `name`.
///
/// An integer is any number with no fractional part, `1.0` included — which is JSON Schema's own
/// definition from draft-06 on, and what a figment `f64` default of a whole number serialises to.
fn is_type(value: &Json, name: &str) -> bool {
    match name {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_number().is_some_and(is_integral),
        _ => false,
    }
}

fn is_integral(number: &Number) -> bool {
    number.is_i64() || number.is_u64() || number.as_f64().is_some_and(|n| n.fract() == 0.0)
}

fn one_of(argument: &Json, value: &Json, at: &str) -> Verdict {
    let Some(choices) = argument.as_array() else {
        return Verdict::Undecided;
    };
    if choices.iter().any(|choice| same(choice, value)) {
        Verdict::Holds
    } else {
        fails(at, &format!("is {value}, which is not one of {argument}"))
    }
}

fn equal(argument: &Json, value: &Json, at: &str) -> Verdict {
    if same(argument, value) {
        Verdict::Holds
    } else {
        fails(at, &format!("is {value}, which is not {argument}"))
    }
}

/// JSON Schema equality, under which `1` and `1.0` are the same number.
fn same(left: &Json, right: &Json) -> bool {
    match (left, right) {
        (Json::Number(left), Json::Number(right)) => compare(left, right) == Some(0),
        (Json::Array(left), Json::Array(right)) => {
            left.len() == right.len() && left.iter().zip(right).all(|(l, r)| same(l, r))
        }
        (Json::Object(left), Json::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(name, l)| right.get(name).is_some_and(|r| same(l, r)))
        }
        _ => left == right,
    }
}

fn negated(argument: &Json, value: &Json, at: &str, depth: usize) -> Verdict {
    match evaluate(argument, value, at, depth + 1) {
        Verdict::Holds => fails(at, &format!("is {value}, which the schema excludes")),
        Verdict::Fails(_) => Verdict::Holds,
        // The reason the verdict has three values: a subschema nothing could decide is not one that
        // holds, and reading it as one would refuse a value on the strength of a guess.
        Verdict::Undecided => Verdict::Undecided,
    }
}

/// `-1`, `0` or `1` as `left` is below, equal to or above `right`, exactly where both are
/// integers and in `f64` otherwise.
fn compare(left: &Number, right: &Number) -> Option<i8> {
    let ordering = if let (Some(l), Some(r)) = (left.as_i64(), right.as_i64()) {
        l.cmp(&r)
    } else if let (Some(l), Some(r)) = (left.as_u64(), right.as_u64()) {
        l.cmp(&r)
    } else {
        left.as_f64()?.partial_cmp(&right.as_f64()?)?
    };
    Some(match ordering {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })
}

fn bound(keyword: &str, argument: &Json, value: &Json, at: &str) -> Verdict {
    // A bound constrains numbers and lets everything else past; `type` is what refuses a string.
    let Some(number) = value.as_number() else {
        return Verdict::Holds;
    };
    let (Some(limit), Some(ordering)) = (
        argument.as_number(),
        argument
            .as_number()
            .and_then(|limit| compare(number, limit)),
    ) else {
        return Verdict::Undecided;
    };
    let (outside, phrasing) = match keyword {
        "minimum" => (ordering < 0, "below the minimum"),
        "maximum" => (ordering > 0, "above the maximum"),
        "exclusiveMinimum" => (ordering <= 0, "not above the exclusive minimum"),
        _ => (ordering >= 0, "not below the exclusive maximum"),
    };
    if outside {
        fails(at, &format!("is {value}, {phrasing} {limit}"))
    } else {
        Verdict::Holds
    }
}

fn length(keyword: &str, argument: &Json, value: &Json, at: &str) -> Verdict {
    let Some(text) = value.as_str() else {
        return Verdict::Holds;
    };
    let Some(limit) = argument.as_u64() else {
        return Verdict::Undecided;
    };
    let length = text.chars().count() as u64;
    match keyword {
        "minLength" if length < limit => fails(
            at,
            &format!("is {value}, shorter than {limit} character(s)"),
        ),
        "maxLength" if length > limit => {
            fails(at, &format!("is {value}, longer than {limit} character(s)"))
        }
        _ => Verdict::Holds,
    }
}

/// `pattern`, for a pattern inside the portable subset — the only patterns this crate can promise
/// every other engine reads as it does. Any other is [`Verdict::Undecided`].
fn pattern(argument: &Json, value: &Json, at: &str) -> Verdict {
    let Some(text) = value.as_str() else {
        return Verdict::Holds;
    };
    let Some(matcher) = argument.as_str().and_then(super::pattern::matcher) else {
        return Verdict::Undecided;
    };
    if matcher.is_match(text) {
        Verdict::Holds
    } else {
        fails(at, &format!("is {value}, which does not match {argument}"))
    }
}

fn count(keyword: &str, argument: &Json, value: &Json, at: &str) -> Verdict {
    let Some(items) = value.as_array() else {
        return Verdict::Holds;
    };
    let Some(limit) = argument.as_u64() else {
        return Verdict::Undecided;
    };
    let held = items.len() as u64;
    match keyword {
        "minItems" if held < limit => fails(at, &format!("has {held} item(s), fewer than {limit}")),
        "maxItems" if held > limit => fails(at, &format!("has {held} item(s), more than {limit}")),
        _ => Verdict::Holds,
    }
}

fn unique(argument: &Json, value: &Json, at: &str) -> Verdict {
    let (Some(items), Some(true)) = (value.as_array(), argument.as_bool()) else {
        return Verdict::Holds;
    };
    for (index, item) in items.iter().enumerate() {
        if items[..index].iter().any(|earlier| same(earlier, item)) {
            return fails(
                at,
                &format!("repeats {item}, and every item has to be distinct"),
            );
        }
    }
    Verdict::Holds
}

fn items(argument: &Json, value: &Json, at: &str, depth: usize) -> Verdict {
    let Some(held) = value.as_array() else {
        return Verdict::Holds;
    };
    // The tuple form is a list of schemas, which no producer here emits.
    if !argument.is_object() {
        return Verdict::Undecided;
    }
    let mut result = Verdict::Holds;
    for (index, item) in held.iter().enumerate() {
        result = result.and(evaluate(
            argument,
            item,
            &format!("{at}[{index}]"),
            depth + 1,
        ));
        if matches!(result, Verdict::Fails(_)) {
            break;
        }
    }
    result
}

/// Every missing name at once, in the order the schema lists them: a message naming one missing
/// entry of three is a second round trip through whatever reported it.
fn required(argument: &Json, value: &Json, at: &str) -> Verdict {
    let Some(fields) = value.as_object() else {
        return Verdict::Holds;
    };
    let Some(names) = argument.as_array() else {
        return Verdict::Undecided;
    };
    let missing: Vec<String> = names
        .iter()
        .filter_map(Json::as_str)
        .filter(|name| !fields.contains_key(*name))
        .map(|name| format!("`{name}`"))
        .collect();
    match missing.as_slice() {
        [] => Verdict::Holds,
        [one] => fails(at, &format!("is missing the required entry {one}")),
        many => fails(
            at,
            &format!("is missing the required entries {}", many.join(", ")),
        ),
    }
}

fn properties(argument: &Json, value: &Json, at: &str, depth: usize) -> Verdict {
    let (Some(fields), Some(schemas)) = (value.as_object(), argument.as_object()) else {
        return if value.is_object() {
            Verdict::Undecided
        } else {
            Verdict::Holds
        };
    };
    let mut result = Verdict::Holds;
    for (name, schema) in schemas {
        if let Some(held) = fields.get(name) {
            result = result.and(evaluate(schema, held, &member(at, name), depth + 1));
            if matches!(result, Verdict::Fails(_)) {
                break;
            }
        }
    }
    result
}

/// `additionalProperties`, which reads its sibling `properties` to know what is additional.
fn additional(
    schema: &Map<String, Json>,
    argument: &Json,
    value: &Json,
    at: &str,
    depth: usize,
) -> Verdict {
    let Some(fields) = value.as_object() else {
        return Verdict::Holds;
    };
    let declared = schema.get("properties").and_then(Json::as_object);
    let extra = fields
        .iter()
        .filter(|(name, _)| declared.is_none_or(|declared| !declared.contains_key(*name)));

    let mut result = Verdict::Holds;
    for (name, held) in extra {
        let found = match argument {
            Json::Bool(true) => Verdict::Holds,
            Json::Bool(false) => fails(&member(at, name), "is not a key this table declares"),
            Json::Object(_) => evaluate(argument, held, &member(at, name), depth + 1),
            _ => Verdict::Undecided,
        };
        result = result.and(found);
        if matches!(result, Verdict::Fails(_)) {
            break;
        }
    }
    result
}

/// `propertyNames`: every entry name, as a string, against the schema.
fn names(argument: &Json, value: &Json, at: &str, depth: usize) -> Verdict {
    let Some(fields) = value.as_object() else {
        return Verdict::Holds;
    };
    let mut result = Verdict::Holds;
    for name in fields.keys() {
        let found = match evaluate(argument, &Json::String(name.clone()), "", depth + 1) {
            Verdict::Fails(reason) => fails(at, &format!("has an entry name that {reason}")),
            other => other,
        };
        result = result.and(found);
        if matches!(result, Verdict::Fails(_)) {
            break;
        }
    }
    result
}

fn all_of(argument: &Json, value: &Json, at: &str, depth: usize) -> Verdict {
    let Some(schemas) = argument.as_array() else {
        return Verdict::Undecided;
    };
    let mut result = Verdict::Holds;
    for schema in schemas {
        result = result.and(evaluate(schema, value, at, depth + 1));
        if matches!(result, Verdict::Fails(_)) {
            break;
        }
    }
    result
}

/// Fails only when every branch fails: one undecided branch could be the one that holds.
fn any_of(argument: &Json, value: &Json, at: &str, depth: usize) -> Verdict {
    let Some(schemas) = argument.as_array() else {
        return Verdict::Undecided;
    };
    let mut reasons = Vec::new();
    for schema in schemas {
        match evaluate(schema, value, at, depth + 1) {
            Verdict::Holds => return Verdict::Holds,
            Verdict::Undecided => return Verdict::Undecided,
            Verdict::Fails(reason) => reasons.push(reason),
        }
    }
    if reasons.is_empty() {
        // `anyOf: []` is not a schema any producer writes, and not one a value can satisfy.
        return Verdict::Undecided;
    }
    Verdict::Fails(reasons.join("; and "))
}

/// A field's position below `at`, spelled as a dotted path.
fn member(at: &str, name: &str) -> String {
    if at.is_empty() {
        name.to_owned()
    } else {
        format!("{at}.{name}")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Verdict, verdict};

    fn fails(constraint: &serde_json::Value, value: &serde_json::Value) -> String {
        match verdict(constraint, value) {
            Verdict::Fails(reason) => reason,
            other => panic!("{value} against {constraint} was {other:?}, not a failure"),
        }
    }

    #[test]
    fn a_map_missing_required_entries_names_every_one() {
        let constraint = json!({"type": "object", "required": ["imprint", "privacy"]});
        assert_eq!(
            fails(&constraint, &json!({})),
            "is missing the required entries `imprint`, `privacy`"
        );
        assert_eq!(
            fails(&constraint, &json!({"privacy": {}})),
            "is missing the required entry `imprint`"
        );
        assert_eq!(
            verdict(
                &constraint,
                &json!({"imprint": 1, "privacy": 2, "terms": 3})
            ),
            Verdict::Holds
        );
    }

    #[test]
    fn every_keyword_a_derive_emits_is_decided() {
        let cases = [
            (
                json!({"type": "integer", "minimum": 0, "maximum": 65535}),
                json!(8080),
            ),
            (
                json!({"type": "number", "minimum": 0.0, "maximum": 1.0}),
                json!(0.0),
            ),
            (
                json!({"type": "string", "minLength": 1, "maxLength": 1}),
                json!("x"),
            ),
            (json!({"type": "string", "enum": ["a", "b"]}), json!("a")),
            (json!({"type": "integer", "not": {"const": 0}}), json!(3)),
            (
                json!({"type": "array", "uniqueItems": true, "minItems": 1, "maxItems": 2,
                       "items": {"type": "string"}}),
                json!(["a"]),
            ),
            (
                json!({"type": "object", "additionalProperties": {"type": "object",
                       "properties": {"id": {"type": "string", "description": "x"}},
                       "required": ["id"], "additionalProperties": false}}),
                json!({"one": {"id": "1"}}),
            ),
        ];
        for (constraint, value) in cases {
            assert_eq!(verdict(&constraint, &value), Verdict::Holds, "{constraint}");
        }
    }

    #[test]
    fn a_failure_names_its_position() {
        let constraint = json!({"type": "object", "additionalProperties":
            {"type": "array", "items": {"type": "integer", "minimum": 0}}});
        assert_eq!(
            fails(&constraint, &json!({"a": [1, -1]})),
            "`a[1]` is -1, below the minimum 0"
        );
    }

    #[test]
    fn a_whole_float_is_an_integer_and_equal_to_one() {
        assert_eq!(
            verdict(&json!({"type": "integer"}), &json!(1.0)),
            Verdict::Holds
        );
        assert_eq!(verdict(&json!({"enum": [1]}), &json!(1.0)), Verdict::Holds);
    }

    #[test]
    fn an_unknown_keyword_is_undecided_and_never_satisfies_a_not() {
        let unknown = json!({"format": "email"});
        assert_eq!(verdict(&unknown, &json!("b")), Verdict::Undecided);
        // Read as holding, the `not` would refuse "b" on the strength of a keyword nobody checked.
        assert_eq!(
            verdict(&json!({"not": unknown}), &json!("b")),
            Verdict::Undecided
        );
        // A certain failure still wins beside it.
        let mixed = json!({"type": "integer", "format": "email"});
        assert!(matches!(verdict(&mixed, &json!("b")), Verdict::Fails(_)));
    }

    #[test]
    fn entry_names_are_held_to_the_pattern_and_a_foreign_pattern_is_undecided() {
        let slugs = json!({"type": "object", "propertyNames": {"pattern": "^[a-z]+$"}});
        assert_eq!(verdict(&slugs, &json!({"terms": 1})), Verdict::Holds);
        assert_eq!(
            fails(&slugs, &json!({"terms": 1, "Privacy": 2})),
            "has an entry name that is \"Privacy\", which does not match \"^[a-z]+$\""
        );
        assert_eq!(
            fails(
                &json!({"additionalProperties": slugs}),
                &json!({"legal": {"X": 1}})
            ),
            "`legal` has an entry name that is \"X\", which does not match \"^[a-z]+$\""
        );
        // `\d` is outside the portable subset, so nothing here is certain about it.
        assert_eq!(
            verdict(&json!({"pattern": r"^\d+$"}), &json!("x")),
            Verdict::Undecided
        );
    }

    #[test]
    fn any_of_fails_only_when_every_branch_does() {
        let either = json!({"anyOf": [{"required": ["user"]}, {"required": ["username"]}]});
        assert_eq!(verdict(&either, &json!({"user": "x"})), Verdict::Holds);
        assert!(matches!(verdict(&either, &json!({})), Verdict::Fails(_)));
    }
}
