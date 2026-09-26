//! Conditions between the fields of a struct, as a typed vocabulary rather than a schema fragment.
//!
//! Some rules relate one field to another: a legal document is hosted *or* external, never both
//! and never neither; a consent requirement other than `none` needs a version. JSON Schema says
//! these with `oneOf` and `if`/`then`, and a raw fragment could say them — along with anything
//! else, including something that widens what the type stated. [`Condition`] is the closed set of
//! shapes a refinement may publish instead: each is checked against the struct it is stated on, and
//! each has a readable form every rendering shows.
//!
//! A condition published by [`Refinement::Holds`](super::Refinement::Holds) is a conjunct added to
//! what the type stated, so it can only tighten. Whether it describes the runtime check *exactly*
//! is the publisher's to get right; what this module guarantees is that it names only fields the
//! struct declares, and asks of each only what that field's type can answer.

use serde_json::{Map, Number, Value as Json};

use super::check::{self, Verdict};
use super::{MAX_DEPTH, pattern};

/// One condition on the fields of a struct.
///
/// A field is named by its path **relative to the struct the condition is stated on**, dotted
/// through nested structs: `url`, or `consent.requirement`. Every segment must be a field the
/// struct declares.
///
/// "Set" means what the loader means by it: the field is present in the document. A field whose
/// type is `Option<T>` and holds `None` is absent, and one holding a value is present — a document
/// cannot spell a present `None`, since the field's own constraint does not admit `null`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Condition {
    /// The field is set.
    Present(String),
    /// The field is not set.
    Absent(String),
    /// The field is set and not empty: a map or struct with at least one entry, a sequence with at
    /// least one item, a string with at least one character.
    NonEmpty(String),
    /// The field is not set, or set and empty — the negation of [`Self::NonEmpty`].
    Empty(String),
    /// The field is set and equal to the value.
    Equals(String, Json),
    /// The field is set and not equal to the value.
    NotEquals(String, Json),
    /// The field is set and a number strictly above the bound.
    Above(String, Number),
    /// The field is set and a string matching the pattern, which lies inside the portable subset.
    Matches(String, String),
    /// Every condition holds.
    All(Vec<Condition>),
    /// Exactly one condition holds.
    ExactlyOne(Vec<Condition>),
    /// Whenever the first holds, so does the second. The second may itself be a `When`, or an
    /// [`Self::All`] holding one, which is how a rule that only applies under another is said.
    When(Box<Condition>, Box<Condition>),
}

impl Condition {
    /// [`Self::Present`].
    #[must_use]
    pub fn present(field: impl Into<String>) -> Self {
        Self::Present(field.into())
    }

    /// [`Self::Absent`].
    #[must_use]
    pub fn absent(field: impl Into<String>) -> Self {
        Self::Absent(field.into())
    }

    /// [`Self::NonEmpty`].
    #[must_use]
    pub fn non_empty(field: impl Into<String>) -> Self {
        Self::NonEmpty(field.into())
    }

    /// [`Self::Empty`].
    #[must_use]
    pub fn empty(field: impl Into<String>) -> Self {
        Self::Empty(field.into())
    }

    /// [`Self::Equals`].
    #[must_use]
    pub fn equals(field: impl Into<String>, value: impl Into<Json>) -> Self {
        Self::Equals(field.into(), value.into())
    }

    /// [`Self::NotEquals`].
    #[must_use]
    pub fn not_equals(field: impl Into<String>, value: impl Into<Json>) -> Self {
        Self::NotEquals(field.into(), value.into())
    }

    /// [`Self::Above`].
    #[must_use]
    pub fn above(field: impl Into<String>, bound: impl Into<Number>) -> Self {
        Self::Above(field.into(), bound.into())
    }

    /// [`Self::Matches`].
    #[must_use]
    pub fn matches(field: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self::Matches(field.into(), pattern.into())
    }

    /// [`Self::All`].
    #[must_use]
    pub fn all(conditions: impl IntoIterator<Item = Self>) -> Self {
        Self::All(conditions.into_iter().collect())
    }

    /// [`Self::ExactlyOne`].
    #[must_use]
    pub fn exactly_one(conditions: impl IntoIterator<Item = Self>) -> Self {
        Self::ExactlyOne(conditions.into_iter().collect())
    }

    /// [`Self::When`].
    #[must_use]
    pub fn when(condition: Self, then: Self) -> Self {
        Self::When(Box::new(condition), Box::new(then))
    }
}

/// A condition checked against the struct it is stated on: the JSON Schema that says it, and the
/// sentence every rendering shows for it.
pub(super) struct Stated {
    pub(super) schema: Map<String, Json>,
    pub(super) description: String,
}

/// `condition` against the struct schema `target` at `at`, or why it cannot be stated there.
pub(super) fn state(
    condition: &Condition,
    target: &Map<String, Json>,
    at: &str,
) -> Result<Stated, String> {
    let schema = schema(condition, target, at, 0)?;
    Ok(Stated {
        schema,
        description: describe(condition),
    })
}

fn schema(
    condition: &Condition,
    target: &Map<String, Json>,
    at: &str,
    depth: usize,
) -> Result<Map<String, Json>, String> {
    if depth > MAX_DEPTH {
        return Err(format!("conditions nest more than {MAX_DEPTH} deep"));
    }
    let nested = |conditions: &[Condition]| -> Result<Vec<Json>, String> {
        conditions
            .iter()
            .map(|condition| schema(condition, target, at, depth + 1).map(Json::Object))
            .collect()
    };
    Ok(match condition {
        Condition::Present(path) => {
            field(target, at, path)?;
            wrap(path, Map::new(), true)
        }
        Condition::Absent(path) => {
            field(target, at, path)?;
            absent(path)
        }
        Condition::NonEmpty(path) | Condition::Empty(path) => {
            let held = field(target, at, path)?;
            let non_empty = matches!(condition, Condition::NonEmpty(_));
            let keyword = emptiness(held, path, non_empty)?;
            let bound = u64::from(non_empty);
            wrap(path, one(keyword, Json::from(bound)), non_empty)
        }
        Condition::Equals(path, value) | Condition::NotEquals(path, value) => {
            let held = field(target, at, path)?;
            if let Verdict::Fails(why) = check::verdict(&Json::Object(held.clone()), value) {
                return Err(format!(
                    "`{path}` can never be {value}, which its own constraint rejects: {why}"
                ));
            }
            let leaf = if matches!(condition, Condition::Equals(..)) {
                one("const", value.clone())
            } else {
                one("not", Json::Object(one("const", value.clone())))
            };
            wrap(path, leaf, true)
        }
        Condition::Above(path, bound) => {
            let held = field(target, at, path)?;
            if !is_type(held, &["integer", "number"]) {
                return Err(format!(
                    "`{path}` is not a number, so it has nothing to be above {bound}: its \
                     constraint is {}",
                    Json::Object(held.clone())
                ));
            }
            wrap(
                path,
                one("exclusiveMinimum", Json::Number(bound.clone())),
                true,
            )
        }
        Condition::Matches(path, source) => {
            let held = field(target, at, path)?;
            if !is_type(held, &["string"]) {
                return Err(format!(
                    "`{path}` is not a string, so it has no text for a pattern to match: its \
                     constraint is {}",
                    Json::Object(held.clone())
                ));
            }
            pattern::check(source).map_err(|why| {
                format!(
                    "`{source}` cannot be matched against `{path}`: {why}. See *Portable patterns* \
                     in spec/v1/FORMAT.md"
                )
            })?;
            wrap(path, one("pattern", Json::String(source.clone())), true)
        }
        Condition::All(conditions) => {
            if conditions.is_empty() {
                return Err("`all` of no conditions states nothing".to_owned());
            }
            if let [only] = conditions.as_slice() {
                return schema(only, target, at, depth + 1);
            }
            one("allOf", Json::Array(nested(conditions)?))
        }
        Condition::ExactlyOne(conditions) => {
            if conditions.len() < 2 {
                return Err(
                    "`exactly one` needs at least two conditions to choose between".to_owned(),
                );
            }
            one("oneOf", Json::Array(nested(conditions)?))
        }
        Condition::When(condition, then) => {
            let mut when = one(
                "if",
                Json::Object(schema(condition, target, at, depth + 1)?),
            );
            when.insert(
                "then".to_owned(),
                Json::Object(schema(then, target, at, depth + 1)?),
            );
            when
        }
    })
}

/// The schema of the field `path` names below the struct at `at`.
fn field<'a>(
    target: &'a Map<String, Json>,
    at: &str,
    path: &str,
) -> Result<&'a Map<String, Json>, String> {
    let mut schema = target;
    let mut reached = at.to_owned();
    for segment in path.split('.') {
        let declared = schema.get("properties").and_then(Json::as_object);
        let Some(Json::Object(next)) = declared.and_then(|fields| fields.get(segment)) else {
            let fields = declared.map_or_else(
                || " It declares no fields.".to_owned(),
                |fields| {
                    format!(
                        " Its fields are {}.",
                        fields
                            .keys()
                            .map(|name| format!("`{name}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                },
            );
            return Err(format!(
                "`{segment}` is not a field of `{reached}`, so a condition on `{path}` names \
                 nothing.{fields}"
            ));
        };
        schema = next;
        reached.push('.');
        reached.push_str(segment);
    }
    Ok(schema)
}

/// The keyword that says a field of this shape is empty (`non_empty` false) or not.
fn emptiness(
    held: &Map<String, Json>,
    path: &str,
    non_empty: bool,
) -> Result<&'static str, String> {
    let keywords = if is_type(held, &["object"]) {
        ("minProperties", "maxProperties")
    } else if is_type(held, &["array"]) {
        ("minItems", "maxItems")
    } else if is_type(held, &["string"]) {
        ("minLength", "maxLength")
    } else {
        return Err(format!(
            "`{path}` is neither a map, a sequence nor a string, so it has no emptiness: its \
             constraint is {}",
            Json::Object(held.clone())
        ));
    };
    Ok(if non_empty { keywords.0 } else { keywords.1 })
}

fn is_type(held: &Map<String, Json>, names: &[&str]) -> bool {
    match held.get("type") {
        Some(Json::String(name)) => names.contains(&name.as_str()),
        Some(Json::Array(declared)) => declared
            .iter()
            .filter_map(Json::as_str)
            .any(|name| names.contains(&name)),
        _ => false,
    }
}

fn one(keyword: &str, value: Json) -> Map<String, Json> {
    let mut schema = Map::new();
    schema.insert(keyword.to_owned(), value);
    schema
}

/// `leaf` at `path`, nested through each segment, every segment required when `present`.
fn wrap(path: &str, leaf: Map<String, Json>, present: bool) -> Map<String, Json> {
    let mut inner = leaf;
    for segment in path.split('.').rev() {
        let mut level = Map::new();
        if present {
            level.insert(
                "required".to_owned(),
                Json::Array(vec![Json::from(segment)]),
            );
        }
        if !inner.is_empty() {
            level.insert(
                "properties".to_owned(),
                Json::Object(one(segment, Json::Object(inner))),
            );
        }
        inner = level;
    }
    inner
}

/// The field at `path` is not set: its parent, if present, does not hold it.
fn absent(path: &str) -> Map<String, Json> {
    let (parents, last) = path.rsplit_once('.').unwrap_or(("", path));
    let leaf = one(
        "not",
        Json::Object(one("required", Json::Array(vec![Json::from(last)]))),
    );
    if parents.is_empty() {
        leaf
    } else {
        wrap(parents, leaf, false)
    }
}

/// The sentence every rendering shows for `condition`.
///
/// Field paths in backticks, values as JSON, and every compound bracketed so that nesting cannot be
/// misread: `when … : all of: [a; b]`.
pub(super) fn describe(condition: &Condition) -> String {
    let list = |conditions: &[Condition]| {
        conditions
            .iter()
            .map(describe)
            .collect::<Vec<_>>()
            .join("; ")
    };
    match condition {
        Condition::Present(path) => format!("`{path}` is set"),
        Condition::Absent(path) => format!("`{path}` is not set"),
        Condition::NonEmpty(path) => format!("`{path}` is not empty"),
        Condition::Empty(path) => format!("`{path}` is empty or not set"),
        Condition::Equals(path, value) => format!("`{path}` is {value}"),
        Condition::NotEquals(path, value) => format!("`{path}` is set and not {value}"),
        Condition::Above(path, bound) => format!("`{path}` is above {bound}"),
        Condition::Matches(path, source) => format!("`{path}` matches `{source}`"),
        Condition::All(conditions) => match conditions.as_slice() {
            [only] => describe(only),
            _ => format!("all of: [{}]", list(conditions)),
        },
        Condition::ExactlyOne(conditions) => format!("exactly one of: [{}]", list(conditions)),
        Condition::When(condition, then) => {
            format!("when {}: {}", describe(condition), describe(then))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value as Json, json};

    use super::{Condition, describe, state};

    fn document() -> Map<String, Json> {
        json!({
            "type": "object",
            "properties": {
                "body": {"type": "object", "additionalProperties": {"type": "string"}},
                "url": {"type": "string"},
                "consent": {
                    "type": "object",
                    "properties": {
                        "requirement": {"type": "string", "enum": ["none", "accept"]},
                        "version": {"type": "string"},
                        "grace_days": {"type": "integer", "minimum": 0, "maximum": 365},
                        "effective": {"type": "string"},
                    },
                },
            },
        })
        .as_object()
        .cloned()
        .expect("an object")
    }

    #[test]
    fn exactly_one_of_hosted_and_external_is_the_issues_encoding_without_the_overlap() {
        let stated = state(
            &Condition::exactly_one([Condition::present("url"), Condition::non_empty("body")]),
            &document(),
            "legal.documents.*",
        )
        .expect("both fields are declared");
        assert_eq!(
            Json::Object(stated.schema),
            json!({"oneOf": [
                {"required": ["url"]},
                {"required": ["body"], "properties": {"body": {"minProperties": 1}}},
            ]})
        );
        assert_eq!(
            stated.description,
            "exactly one of: [`url` is set; `body` is not empty]"
        );
    }

    #[test]
    fn a_consent_rule_nests_its_grace_rule_inside_its_consequence() {
        let rule = Condition::when(
            Condition::not_equals("consent.requirement", "none"),
            Condition::all([
                Condition::absent("url"),
                Condition::matches("consent.version", "[^ ]"),
                Condition::when(
                    Condition::above("consent.grace_days", 0),
                    Condition::present("consent.effective"),
                ),
            ]),
        );
        let stated =
            state(&rule, &document(), "legal.documents.*").expect("every field is declared");
        assert_eq!(
            Json::Object(stated.schema),
            json!({
                "if": {"required": ["consent"], "properties": {"consent": {
                    "required": ["requirement"],
                    "properties": {"requirement": {"not": {"const": "none"}}}}}},
                "then": {"allOf": [
                    {"not": {"required": ["url"]}},
                    {"required": ["consent"], "properties": {"consent": {
                        "required": ["version"], "properties": {"version": {"pattern": "[^ ]"}}}}},
                    {"if": {"required": ["consent"], "properties": {"consent": {
                        "required": ["grace_days"],
                        "properties": {"grace_days": {"exclusiveMinimum": 0}}}}},
                     "then": {"required": ["consent"], "properties": {"consent": {
                        "required": ["effective"]}}}},
                ]},
            })
        );
        assert_eq!(
            stated.description,
            "when `consent.requirement` is set and not \"none\": all of: [`url` is not set; \
             `consent.version` matches `[^ ]`; when `consent.grace_days` is above 0: \
             `consent.effective` is set]"
        );
    }

    #[test]
    fn a_nested_absence_does_not_require_its_parent() {
        let stated =
            state(&Condition::absent("consent.version"), &document(), "d").expect("declared");
        assert_eq!(
            Json::Object(stated.schema),
            json!({"properties": {"consent": {"not": {"required": ["version"]}}}})
        );
        assert_eq!(
            describe(&Condition::empty("body")),
            "`body` is empty or not set"
        );
    }

    #[test]
    fn a_condition_asks_only_what_the_field_can_answer() {
        for (condition, reason) in [
            (
                Condition::present("urll"),
                "`urll` is not a field of `d`, so a condition on `urll` names nothing. Its fields \
                 are `body`, `consent`, `url`.",
            ),
            (
                Condition::present("consent.versoin"),
                "`versoin` is not a field of `d.consent`",
            ),
            (Condition::above("url", 0), "`url` is not a number"),
            (Condition::matches("body", "x"), "`body` is not a string"),
            (
                Condition::matches("url", r"\S"),
                "cannot be matched against `url`",
            ),
            (
                Condition::equals("consent.requirement", "always"),
                "`consent.requirement` can never be \"always\"",
            ),
            (
                Condition::non_empty("consent.grace_days"),
                "has no emptiness",
            ),
            (
                Condition::exactly_one([Condition::present("url")]),
                "at least two conditions",
            ),
            (Condition::all([]), "states nothing"),
        ] {
            let refusal = state(&condition, &document(), "d")
                .err()
                .unwrap_or_else(|| panic!("{condition:?} was accepted"));
            assert!(refusal.contains(reason), "{condition:?}: {refusal}");
        }
    }
}
