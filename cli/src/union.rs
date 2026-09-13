//! Several images, one document.
//!
//! A configuration document may be read by more than one binary, and each of their contracts covers
//! only the keys its own binary consumes. Validating that document against one of them with
//! `additionalProperties: false` would therefore reject a perfectly correct deployment: every key
//! belonging to the others would be "unknown". The object to validate against is the union.
//!
//! # Every field must agree, and that is a catch-all on purpose
//!
//! | Situation | Rule |
//! |---|---|
//! | a path in one contract only | keep |
//! | a path in several, identical | keep once |
//! | `required` | union — any reader requiring it wins |
//! | `additionalProperties` | `false` at every enumerated level |
//! | **any other field or keyword, differing** | **refused** |
//!
//! Naming `type` and `enum` and leaving the rest to last-one-wins is a rule nobody wrote down: two
//! images disagreeing about a key's `maximum` is the same defect as disagreeing about its `type` —
//! one contract accepts a value the other refuses — and enumerating the fields that matter means
//! the next field a producer adds falls through the gap in silence.
//!
//! That is also why an entry is held as raw JSON rather than as [`Key`](crate::Key). A struct drops
//! what it has not learned, and a dropped field is one two contracts can disagree about without
//! anything noticing. The typed view is [`crate::value::Entry`], which is a window onto the map
//! rather than a replacement for it.
//!
//! # It is the identity today, and it still is not the same question
//!
//! Every document declared in the corpus this was ported against binds a single image, so the union
//! merges nothing. What it earns is the shape a second reader can be added to without a rule
//! changing — and the distinction from the per-container scope, which is a different question
//! rather than a stricter version of the same one. Gate 1 asks what a *file* may contain; gates 2
//! and 3 ask what one *process* may be handed.

use std::collections::HashMap;

use serde_json::{Map, Value as Json};

use crate::error::Error;
use crate::value::Entry;

/// `external.unknown` policies, weakest first.
///
/// A union takes the strictest: the document and the pod are shared, so a variable any reader
/// refuses is a variable the deployment cannot carry.
pub const UNKNOWN_POLICIES: [&str; 3] = ["allow", "warn", "reject"];

/// The one field that unions rather than having to agree.
const UNIONED_FIELDS: [&str; 1] = ["required"];

/// One merged entry, and which contracts described it.
///
/// The sources are held beside the fields rather than inside them. The implementation this was
/// ported from threaded an `_sources` key through the JSON and stripped it again before validating,
/// which meant every reader of an entry had to know about a field that was never part of the
/// format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    /// The name this entry is keyed on: a key's `path`, a variable's `env` or `name`.
    pub name: String,
    /// The entry as the contracts published it.
    pub fields: Map<String, Json>,
    /// The labels of the contracts that described it, in merge order.
    pub sources: Vec<String>,
}

impl Merged {
    /// The typed window the value checks read this through.
    pub fn entry(&self) -> Entry<'_> {
        Entry(&self.fields)
    }

    /// One string field.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.fields.get(name).and_then(Json::as_str)
    }
}

/// Entries by name, in the order the contracts declared them.
///
/// Declaration order is load-bearing in two places — the first key whose `env` matches a variable
/// wins, and so does the first `structured` key a name extends — so a sorted map would quietly
/// change which key a message names. The index is what keeps the lookups from being a scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ordered {
    entries: Vec<Merged>,
    index: HashMap<String, usize>,
}

impl Ordered {
    /// One entry by the name it is keyed on.
    pub fn get(&self, name: &str) -> Option<&Merged> {
        self.index
            .get(name)
            .map(|position| &self.entries[*position])
    }

    /// Whether an entry is keyed on this name.
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// Every entry, in declaration order.
    pub fn iter(&self) -> std::slice::Iter<'_, Merged> {
        self.entries.iter()
    }

    /// Every name, in declaration order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|entry| entry.name.as_str())
    }

    /// How many entries there are.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<'a> IntoIterator for &'a Ordered {
    type Item = &'a Merged;
    type IntoIter = std::slice::Iter<'a, Merged>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// The contracts of every image that reads one document, merged into one description.
#[derive(Debug, Clone, Default)]
pub struct Union {
    /// The labels of the contracts merged, in order.
    pub sources: Vec<String>,
    /// The spelling rules every reader agrees on. Held whole, because two images reading one
    /// document under different rules do not share a namespace at all.
    pub dialect: Map<String, Json>,
    /// The variables the loaders read before the layers exist, keyed by `env`.
    pub loader: Ordered,
    /// Every key, keyed by `path`.
    pub keys: Ordered,
    /// Every declared external variable, keyed by `name`.
    pub external_env: Ordered,
    /// The ignore patterns, sorted and deduplicated.
    pub ignore: Vec<String>,
    /// The strictest policy any reader declared.
    pub unknown: String,
    /// The merged JSON Schema, closed at every enumerated level.
    pub json_schema: Json,
    /// The `producer.loader` every merged contract named, or the empty string when they do not
    /// agree on one — which includes a document that names none at all.
    ///
    /// Held on the union rather than looked up per key, because the range check is a question about
    /// the *document* and asking it once is what keeps one answer from being reported per value.
    pub loader_name: String,
}

impl Union {
    /// The namespace every spelling starts with.
    pub fn prefix(&self) -> &str {
        self.dialect
            .get("prefix")
            .and_then(Json::as_str)
            .unwrap_or_default()
    }

    /// What is appended to name a file holding a value instead.
    pub fn indirection_suffix(&self) -> &str {
        self.dialect
            .get("indirection_suffix")
            .and_then(Json::as_str)
            .unwrap_or_default()
    }

    /// What separates one path segment from the next.
    ///
    /// Read by exactly one rule here — [`Self::container_of`], which is a tier 2 assumption and
    /// says so. Everything else reads the spellings the document published rather than re-deriving
    /// one, because a producer that hands naming to a binder with its own relaxed-binding rules
    /// does not reach tier 2 and should not be assumed to.
    pub fn nesting_separator(&self) -> &str {
        self.dialect
            .get("nesting_separator")
            .and_then(Json::as_str)
            .unwrap_or("__")
    }

    /// The key whose `env` / `env_file` / `secrets_file` is `name`, if any.
    pub fn key_by(&self, spelling: &str, name: &str) -> Option<&Merged> {
        self.keys
            .iter()
            .find(|key| key.text(spelling) == Some(name))
    }

    /// Every spelling of one kind the keys publish, for a suggestion.
    pub fn spellings(&self, spelling: &str) -> Vec<&str> {
        self.keys
            .iter()
            .filter_map(|key| key.text(spelling))
            .collect()
    }

    /// The dynamic-map key that `name` addresses a leaf inside, if any.
    ///
    /// A map whose *sub*-keys are chosen by the deployment rather than by the program is a key no
    /// contract generated at build time can enumerate. What the producer can say is that the key is
    /// `structured`, and the loader builds it from nested spellings: a secrets file called
    /// `internal__peers__api__token` supplies `internal.peers.api.token`.
    ///
    /// **This is the one rule here that assumes tier 2**, and the assumption is the separator. It
    /// is true of a loader that derives its spellings the way the dialect describes, and false of
    /// one that hands naming to a binder with its own rules — so the answer is a *legitimising*
    /// one: it can only turn a finding into silence, never silence into a finding. A loader that
    /// does not compose names this way simply has no such keys, and nothing here fires.
    ///
    /// Only `structured` keys qualify. A name extending a scalar key's spelling is still a mistake.
    pub fn container_of(&self, spelling: &str, name: &str) -> Option<&Merged> {
        let separator = self.nesting_separator();
        self.keys.iter().find(|key| {
            let Some(spelt) = key.text(spelling).filter(|spelt| !spelt.is_empty()) else {
                return false;
            };
            if key.text("text_form") != Some("structured") {
                return false;
            }
            let opening = format!("{spelt}{separator}");
            name.starts_with(&opening) && name.len() > opening.len()
        })
    }
}

/// Merge the contracts of every image that reads one document.
///
/// `items` is `(label, contract)` pairs; the label names the vendored file in every message.
///
/// # Errors
/// [`Error::Invalid`] when the list is empty, when two contracts read one document under different
/// dialects, or when any two describe one entry or one schema position differently.
pub fn union_contracts(items: &[(String, Json)]) -> Result<Union, Error> {
    let Some((first_label, first)) = items.first() else {
        return Err(Error::Invalid("no contracts to union".to_owned()));
    };

    let mut merged = Union {
        sources: items.iter().map(|(label, _)| label.clone()).collect(),
        dialect: object(first, &["schema", "dialect"])
            .cloned()
            .unwrap_or_default(),
        unknown: string(first, &["external", "unknown"])
            .unwrap_or("reject")
            .to_owned(),
        loader_name: string(first, &["producer", "loader"])
            .unwrap_or_default()
            .to_owned(),
        ..Union::default()
    };

    let mut ignore: Vec<String> = Vec::new();

    for (label, contract) in items {
        let dialect = object(contract, &["schema", "dialect"])
            .cloned()
            .unwrap_or_default();
        if dialect != merged.dialect {
            return Err(Error::Invalid(format!(
                "{label} and {first_label} read one document under different dialects: {} vs {}",
                Json::Object(dialect),
                Json::Object(merged.dialect.clone())
            )));
        }

        // A document naming no loader, or two naming different ones, both mean the same thing to
        // the range check: nothing here knows how this text becomes a value. Emptied rather than
        // resolved, because picking one of two would apply one loader's reads to the other's
        // document — the failure the registry exists to prevent.
        let loader = string(contract, &["producer", "loader"]).unwrap_or_default();
        if loader != merged.loader_name {
            merged.loader_name = String::new();
        }

        for entry in array(contract, &["schema", "loader"]) {
            merge_entry(&mut merged.loader, entry, "env", label, "loader variable")?;
        }
        for entry in array(contract, &["schema", "keys"]) {
            merge_entry(&mut merged.keys, entry, "path", label, "key")?;
        }
        for entry in array(contract, &["external", "env"]) {
            merge_entry(
                &mut merged.external_env,
                entry,
                "name",
                label,
                "external variable",
            )?;
        }

        for pattern in array(contract, &["external", "ignore"]) {
            if let Some(pattern) = pattern.as_str()
                && !ignore.iter().any(|held| held == pattern)
            {
                ignore.push(pattern.to_owned());
            }
        }

        let policy = string(contract, &["external", "unknown"]).unwrap_or("reject");
        if strictness(policy) > strictness(&merged.unknown) {
            policy.clone_into(&mut merged.unknown);
        }

        let published = contract.get("json_schema").cloned().unwrap_or(Json::Null);
        if let Some(published) = published.as_object() {
            merged.json_schema = Json::Object(merge_schema(
                merged.json_schema.as_object(),
                published,
                label,
                first_label,
                "$",
                true,
            )?);
        }
    }

    ignore.sort_unstable();
    merged.ignore = ignore;
    Ok(merged)
}

/// How strict one `external.unknown` policy is.
fn strictness(policy: &str) -> usize {
    UNKNOWN_POLICIES
        .iter()
        .position(|known| *known == policy)
        .unwrap_or(UNKNOWN_POLICIES.len())
}

/// Merge one key, loader or external entry into the accumulator, keyed by name.
///
/// `required` unions — a key any reader requires must be present. Every other field must agree,
/// including `docs`, `default` and `note`. That is stricter than it looks and it is deliberate: two
/// binaries that document one key differently have either drifted or are describing two different
/// things, and neither is something a merged document should paper over by picking whichever
/// contract happened to be listed first.
fn merge_entry(
    into: &mut Ordered,
    entry: &Json,
    key_field: &str,
    label: &str,
    kind: &str,
) -> Result<(), Error> {
    let Some(fields) = entry.as_object() else {
        return Err(Error::Invalid(format!(
            "{label}: a {kind} is not an object"
        )));
    };
    let Some(name) = fields.get(key_field).and_then(Json::as_str) else {
        return Err(Error::Invalid(format!(
            "{label}: a {kind} has no `{key_field}`"
        )));
    };

    let Some(position) = into.index.get(name).copied() else {
        into.index.insert(name.to_owned(), into.entries.len());
        into.entries.push(Merged {
            name: name.to_owned(),
            fields: fields.clone(),
            sources: vec![label.to_owned()],
        });
        return Ok(());
    };

    let existing = &mut into.entries[position];
    let mut names: Vec<&String> = existing.fields.keys().chain(fields.keys()).collect();
    names.sort_unstable();
    names.dedup();
    for field_name in names {
        if UNIONED_FIELDS.contains(&field_name.as_str()) {
            continue;
        }
        let mine = existing.fields.get(field_name).unwrap_or(&Json::Null);
        let theirs = fields.get(field_name).unwrap_or(&Json::Null);
        if mine != theirs {
            return Err(Error::Invalid(format!(
                "{kind} '{name}' is described differently by {} and {label}: `{field_name}` is \
                 {mine} in one and {theirs} in the other",
                existing.sources.join(" and ")
            )));
        }
    }

    let required = truthy(existing.fields.get("required")) || truthy(fields.get("required"));
    existing
        .fields
        .insert("required".to_owned(), Json::Bool(required));
    existing.sources.push(label.to_owned());
    Ok(())
}

/// Whether a field is present and not false or null.
fn truthy(value: Option<&Json>) -> bool {
    value.is_some_and(|value| value.as_bool().unwrap_or(!value.is_null()))
}

/// Structurally merge two JSON Schema subtrees, closing every enumerated object as it goes.
///
/// The first contract is merged into nothing and takes the same path as every one after it rather
/// than being copied wholesale: a document read by a single image must come out just as closed as
/// one read by several, or a producer that left a level open would leave the gate open there too.
///
/// `close` is false below an *element* schema, and only there. The document's own levels are closed
/// because an unknown key at one of them is a typo nobody can act on — but an element is a type the
/// producer owns, whose deserialiser accepts a field nobody declared unless the type says
/// otherwise. So the producer decides an element's openness and publishes the answer, and closing
/// it here would reject a document the image accepts, which is the one direction to never be wrong
/// in.
fn merge_schema(
    into: Option<&Map<String, Json>>,
    other: &Map<String, Json>,
    label: &str,
    first_label: &str,
    at: &str,
    close: bool,
) -> Result<Map<String, Json>, Error> {
    let mut merged = into.cloned().unwrap_or_default();

    for (keyword, value) in other {
        match keyword.as_str() {
            "properties" => {
                let held = merged.get("properties").and_then(Json::as_object).cloned();
                let subtree =
                    merge_named(held.as_ref(), value, label, first_label, at, ".", close)?;
                merged.insert("properties".to_owned(), Json::Object(subtree));
            }
            "required" => {
                let mut required: Vec<Json> = merged
                    .get("required")
                    .and_then(Json::as_array)
                    .cloned()
                    .unwrap_or_default();
                for name in value.as_array().into_iter().flatten() {
                    if !required.contains(name) {
                        required.push(name.clone());
                    }
                }
                required.sort_unstable_by_key(|name| name.as_str().unwrap_or("").to_owned());
                merged.insert("required".to_owned(), Json::Array(required));
            }
            "definitions" | "$defs" => {
                let held = merged.get(keyword).and_then(Json::as_object).cloned();
                let subtree =
                    merge_named(held.as_ref(), value, label, first_label, at, "#", close)?;
                merged.insert(keyword.clone(), Json::Object(subtree));
            }
            // The open/closed flag. Forced to false below regardless, so an explicit `true` from
            // either side is not a disagreement worth failing on — it is simply overruled.
            "additionalProperties" if !value.is_object() => {}
            "additionalProperties" | "items" => {
                // An element schema, and a different field wearing the same name in the first case.
                // Merged structurally rather than compared whole, for the reason `properties` is:
                // two contracts describing the same map may each name a keyword the other omits,
                // and an equality test would call that a disagreement.
                let held = merged.get(keyword).and_then(Json::as_object).cloned();
                let element = value.as_object().cloned().unwrap_or_default();
                merged.insert(
                    keyword.clone(),
                    Json::Object(merge_schema(
                        held.as_ref(),
                        &element,
                        label,
                        first_label,
                        &format!("{at}.{keyword}"),
                        false,
                    )?),
                );
            }
            _ => {
                if let Some(held) = merged.get(keyword) {
                    if held != value {
                        return Err(Error::Invalid(format!(
                            "{first_label} and {label} disagree about {at}: `{keyword}` is {held} \
                             in one and {value} in the other"
                        )));
                    }
                } else {
                    merged.insert(keyword.clone(), value.clone());
                }
            }
        }
    }

    if close {
        close_object(&mut merged);
    }
    Ok(merged)
}

/// Merge a mapping of named subschemas — `properties`, `definitions`, `$defs` — one name at a time.
///
/// The two call sites differ only in how a position inside them is spelled, so they share this
/// rather than each carrying the same eight lines: a fix to one that missed the other would be a
/// merge rule that held for a schema's properties and not for its definitions.
fn merge_named(
    into: Option<&Map<String, Json>>,
    other: &Json,
    label: &str,
    first_label: &str,
    at: &str,
    separator: &str,
    close: bool,
) -> Result<Map<String, Json>, Error> {
    let mut merged = into.cloned().unwrap_or_default();
    for (name, subschema) in other.as_object().into_iter().flatten() {
        let Some(subschema) = subschema.as_object() else {
            continue;
        };
        let held = merged.get(name).and_then(Json::as_object).cloned();
        merged.insert(
            name.clone(),
            Json::Object(merge_schema(
                held.as_ref(),
                subschema,
                label,
                first_label,
                &format!("{at}{separator}{name}"),
                close,
            )?),
        );
    }
    Ok(merged)
}

/// After the union an unknown property is unknown to *every* reader, so it is refused.
///
/// Only where the properties are enumerated. A map's `additionalProperties` is an element schema
/// rather than the flag, and overwriting it with `false` would turn "every value is one of these"
/// into "no key is allowed at all" — a schema refusing every document the image accepts.
fn close_object(schema: &mut Map<String, Json>) {
    let is_element_schema = schema
        .get("additionalProperties")
        .is_some_and(Json::is_object);
    if schema.contains_key("properties") && !is_element_schema {
        schema.insert("additionalProperties".to_owned(), Json::Bool(false));
    }
}

/// Every `$ref` that would take the validator off this machine.
///
/// A chart's own `values.schema.json` legitimately states Kubernetes types by URL. The *app config*
/// schema must not: an offline gate that resolves a remote reference silently becomes a networked
/// one, and a third party gains a say in what CI accepts.
pub fn local_refs_only(schema: &Json) -> Vec<String> {
    fn walk(schema: &Json, at: &str, found: &mut Vec<String>) {
        match schema {
            Json::Object(fields) => {
                if let Some(reference) = fields.get("$ref").and_then(Json::as_str)
                    && !reference.starts_with('#')
                {
                    found.push(format!("{at}: $ref '{reference}' is not a local reference"));
                }
                for (name, value) in fields {
                    walk(value, &format!("{at}.{name}"), found);
                }
            }
            Json::Array(items) => {
                for (index, value) in items.iter().enumerate() {
                    walk(value, &format!("{at}[{index}]"), found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    walk(schema, "$", &mut found);
    found
}

/// `(did you mean ...?)`, or the empty string when nothing is close enough.
///
/// The union already holds every key path and every variable spelling, so this costs nothing and
/// turns a rename from a puzzle into a one-line answer — which is the whole point on the pull
/// request that bumps a digest.
pub fn suggest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> String {
    let lowered = name.to_lowercase();
    let mut scored: Vec<(usize, &str)> = Vec::new();
    for candidate in candidates {
        if candidate == name {
            continue;
        }
        let distance = levenshtein(&lowered, &candidate.to_lowercase());
        let allowance = std::cmp::max(
            2,
            std::cmp::min(name.chars().count(), candidate.chars().count()) / 3,
        );
        if distance <= allowance {
            scored.push((distance, candidate));
        }
    }
    if scored.is_empty() {
        return String::new();
    }
    scored.sort_unstable();
    let named: Vec<&str> = scored
        .iter()
        .take(3)
        .map(|(_, candidate)| *candidate)
        .collect();
    format!(" (did you mean {}?)", named.join(" or "))
}

/// The edit distance between two strings, in characters.
fn levenshtein(left: &str, right: &str) -> usize {
    if left == right {
        return 0;
    }
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (index, from) in left.chars().enumerate() {
        let mut current = vec![index + 1];
        for (position, to) in right.iter().enumerate() {
            current.push(
                (previous[position + 1] + 1)
                    .min(current[position] + 1)
                    .min(previous[position] + usize::from(from != *to)),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

/// One nested object, by path.
fn object<'a>(document: &'a Json, path: &[&str]) -> Option<&'a Map<String, Json>> {
    let mut current = document;
    for step in path {
        current = current.get(step)?;
    }
    current.as_object()
}

/// One nested string, by path.
fn string<'a>(document: &'a Json, path: &[&str]) -> Option<&'a str> {
    let mut current = document;
    for step in path {
        current = current.get(step)?;
    }
    current.as_str()
}

/// One nested array, by path, empty when absent.
fn array<'a>(document: &'a Json, path: &[&str]) -> &'a [Json] {
    let mut current = document;
    for step in path {
        let Some(next) = current.get(step) else {
            return &[];
        };
        current = next;
    }
    current.as_array().map_or(&[], Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value as Json, json};

    use super::{local_refs_only, suggest, union_contracts};

    fn contract(keys: &Json, json_schema: &Json) -> Json {
        json!({
            "terrace_contract": 1,
            "producer": {"name": "x", "version": "1", "loader": "figment"},
            "app": {"name": "x"},
            "schema": {
                "schema_version": 2,
                "dialect": {"prefix": "X_", "nesting_separator": "__", "indirection_suffix": "_FILE"},
                "loader": [], "keys": keys.clone(),
            },
            "json_schema": json_schema.clone(),
            "external": {"env": [], "ignore": [], "unknown": "reject"},
        })
    }

    fn merge(items: Vec<(&str, Json)>) -> Result<super::Union, crate::Error> {
        let owned: Vec<(String, Json)> = items
            .into_iter()
            .map(|(label, contract)| (label.to_owned(), contract))
            .collect();
        union_contracts(&owned)
    }

    #[test]
    fn one_contract_is_still_closed() {
        // A document read by a single image must come out just as closed as one read by several,
        // or a producer that left a level open would leave the gate open there too.
        let union = merge(vec![(
            "a",
            contract(
                &json!([]),
                &json!({"properties": {"port": {"type": "integer"}}}),
            ),
        )])
        .expect("one contract merges");
        assert_eq!(union.json_schema["additionalProperties"], json!(false));
    }

    #[test]
    fn required_unions_and_everything_else_must_agree() {
        let mine = contract(
            &json!([{"path": "a", "text_form": "text", "required": true}]),
            &json!({}),
        );
        let theirs = contract(
            &json!([{"path": "a", "text_form": "text", "required": false}]),
            &json!({}),
        );
        let union = merge(vec![("a", mine), ("b", theirs)]).expect("`required` unions");
        assert_eq!(
            union.keys.get("a").expect("the key merged").fields["required"],
            json!(true)
        );
    }

    #[test]
    fn a_field_two_contracts_disagree_about_is_refused_whatever_it_is() {
        let mine = contract(
            &json!([{"path": "a", "text_form": "text", "docs": "one"}]),
            &json!({}),
        );
        let theirs = contract(
            &json!([{"path": "a", "text_form": "text", "docs": "two"}]),
            &json!({}),
        );
        let error = merge(vec![("a", mine), ("b", theirs)]).expect_err("`docs` must agree");
        assert!(error.to_string().contains("`docs`"), "{error}");
    }

    #[test]
    fn an_element_schema_is_merged_rather_than_closed() {
        // Closing it would turn "every value is one of these" into "no key is allowed at all",
        // which refuses every document the image accepts.
        let union = merge(vec![(
            "a",
            contract(
                &json!([]),
                &json!({"properties": {"peers": {
                    "type": "object",
                    "additionalProperties": {"properties": {"token": {"type": "string"}}},
                }}}),
            ),
        )])
        .expect("one contract merges");
        let element = &union.json_schema["properties"]["peers"]["additionalProperties"];
        assert!(element.is_object(), "{element}");
        assert_eq!(element.get("additionalProperties"), None, "{element}");
    }

    #[test]
    fn two_dialects_are_not_one_namespace() {
        let mut theirs = contract(&json!([]), &json!({}));
        theirs["schema"]["dialect"]["prefix"] = json!("Y_");
        let error = merge(vec![("a", contract(&json!([]), &json!({}))), ("b", theirs)])
            .expect_err("two dialects cannot read one document");
        assert!(error.to_string().contains("different dialects"), "{error}");
    }

    #[test]
    fn two_loaders_leave_the_union_naming_none() {
        // Picking one would apply that loader's reads to the other's document, which is the failure
        // the registry exists to prevent.
        let mut theirs = contract(&json!([]), &json!({}));
        theirs["producer"]["loader"] = json!("spring-boot");
        let union = merge(vec![("a", contract(&json!([]), &json!({}))), ("b", theirs)])
            .expect("differing loaders still merge");
        assert_eq!(union.loader_name, "");
    }

    #[test]
    fn a_document_with_no_producer_block_still_merges() {
        // Every vendored contract published before the field existed is one of these, and a reader
        // as strict as a writer would refuse a document it should have read.
        let mut without = contract(&json!([]), &json!({}));
        without
            .as_object_mut()
            .expect("an object")
            .remove("producer");
        let union = merge(vec![("a", without)]).expect("a document without a producer reads");
        assert_eq!(union.loader_name, "");
    }

    #[test]
    fn the_strictest_unknown_policy_wins() {
        let mut lenient = contract(&json!([]), &json!({}));
        lenient["external"]["unknown"] = json!("warn");
        let union = merge(vec![
            ("a", lenient),
            ("b", contract(&json!([]), &json!({}))),
        ])
        .expect("policies merge");
        assert_eq!(union.unknown, "reject");
    }

    #[test]
    fn a_structured_key_legitimises_a_name_extending_its_spelling() {
        let union = merge(vec![(
            "a",
            contract(
                &json!([
                    {"path": "peers", "secrets_file": "peers", "text_form": "structured"},
                    {"path": "token", "secrets_file": "token", "text_form": "text"},
                ]),
                &json!({}),
            ),
        )])
        .expect("one contract merges");
        assert!(
            union
                .container_of("secrets_file", "peers__api__token")
                .is_some()
        );
        // A name extending a *scalar* key's spelling is still a mistake.
        assert!(union.container_of("secrets_file", "token__api").is_none());
    }

    #[test]
    fn a_remote_reference_is_reported() {
        let offenders =
            local_refs_only(&json!({"properties": {"a": {"$ref": "https://x/y.json"}}}));
        assert_eq!(offenders.len(), 1, "{offenders:?}");
        assert!(offenders[0].contains("$.properties.a"), "{offenders:?}");
    }

    #[test]
    fn a_local_reference_is_not() {
        assert!(local_refs_only(&json!({"$ref": "#/definitions/a"})).is_empty());
    }

    #[test]
    fn a_near_miss_is_named_and_a_distant_one_is_not() {
        assert_eq!(
            suggest("isr.ttl_sec", ["isr.ttl_secs", "server.port"]),
            " (did you mean isr.ttl_secs?)"
        );
        assert_eq!(suggest("wholly.different", ["server.port"]), "");
    }
}
