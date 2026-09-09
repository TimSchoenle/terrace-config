//! The union of several images' contracts, over fixtures rather than over a registry.
//!
//! A document read by several binaries is the case the union exists for. Each contract covers only
//! the keys its own binary consumes, so validating that document against one of them with
//! `additionalProperties: false` would reject a perfectly correct deployment.
//!
//! No chart in the corpus this was ported against declares one — every document there binds exactly
//! one image — so the merge is the identity in production and these fixtures are the only place its
//! rules run at all. That is a reason to keep them rather than a reason to drop them: a document
//! that gained a second reader lands straight on those rules, and this is the one place where
//! getting them wrong turns a correct chart red.
//!
//! The fixtures are two contracts that share a key (`api`, `worker`), one that describes a shared
//! key differently (`conflicting`), and one whose constraints nest (`deep`). A foreign dialect is
//! `spec/v1/conformance/schema-version-1/`, promoted out of the fixture set because a document under
//! another dialect is something a *consumer* must survive rather than one suite's private business.

#![cfg(feature = "helm")]

use std::path::{Path, PathBuf};

use serde_json::{Value as Json, json};
use terrace_contract::union::{Union, local_refs_only, union_contracts};
use terrace_contract::value::Entry;
use terrace_contract::{Contract, Error};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// One fixture, as a document to be edited before merging.
fn fixture(name: &str) -> Json {
    let path = fixtures().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()));
    serde_json::from_str(&text).expect("a fixture is JSON")
}

/// One case from the conformance corpus, for the documents no fixture holds.
fn case(name: &str) -> Json {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate has a parent directory")
        .join("spec/v1/conformance")
        .join(name)
        .join("contract.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()));
    serde_json::from_str(&text).expect("a case is JSON")
}

fn merge(items: Vec<(&str, Json)>) -> Result<Union, Error> {
    let owned: Vec<(String, Json)> = items
        .into_iter()
        .map(|(label, document)| (label.to_owned(), document))
        .collect();
    union_contracts(&owned)
}

fn union(names: &[&str]) -> Union {
    merge(names.iter().map(|name| (*name, fixture(name))).collect()).expect("the fixtures merge")
}

/// One key of a fixture, for a test that has to change it before merging.
fn key<'a>(document: &'a mut Json, path: &str) -> &'a mut Json {
    document["schema"]["keys"]
        .as_array_mut()
        .expect("a document has keys")
        .iter_mut()
        .find(|entry| entry["path"] == path)
        .unwrap_or_else(|| panic!("the fixture declares {path}"))
}

/// Strip a fixture's keys so a `json_schema` conflict is what the union reports first.
fn schema_only(mut document: Json) -> Json {
    document["schema"]["keys"] = json!([]);
    document
}

// ---------------------------------------------------------------------------------------------
// The keys
// ---------------------------------------------------------------------------------------------

#[test]
fn a_single_contract_is_its_own_union() {
    let merged = union(&["api"]);
    assert!(merged.keys.contains("auth.session_ttl"));
    assert_eq!(merged.prefix(), "FIXTURE_");
}

#[test]
fn keys_from_every_contract_are_kept() {
    let merged = union(&["api", "worker"]);
    let mut names: Vec<&str> = merged.keys.names().collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "auth.session_ttl",
            "database.url",
            "github.repos",
            "log.level",
            "tuning.ratio",
            "worker.concurrency",
            "worker.debug",
        ]
    );
}

#[test]
fn a_shared_key_is_kept_once_and_remembers_who_described_it() {
    let merged = union(&["api", "worker"]);
    let held = merged.keys.get("database.url").expect("the shared key");
    assert_eq!(held.text("ty"), Some("String"));
    assert_eq!(held.sources, ["api", "worker"]);
}

#[test]
fn two_descriptions_of_one_key_is_a_hard_error() {
    let error = merge(vec![
        ("api", fixture("api")),
        ("conflicting", fixture("conflicting")),
    ])
    .expect_err("two descriptions cannot both be right");
    let message = error.to_string();
    for expected in ["auth.session_ttl", "api", "conflicting"] {
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn a_foreign_dialect_is_a_hard_error() {
    // Two images reading one document under different spelling rules do not share a namespace at
    // all, which is the same failure as a disagreeing key one level up.
    let error = merge(vec![
        ("api", fixture("api")),
        ("legacy", case("schema-version-1")),
    ])
    .expect_err("two dialects cannot read one document");
    assert!(error.to_string().contains("dialect"), "{error}");
}

#[test]
fn required_is_unioned() {
    let mut left = fixture("api");
    key(&mut left, "database.url")["required"] = json!(false);
    let merged = merge(vec![("api", left), ("worker", fixture("worker"))])
        .expect("`required` unions rather than having to agree");
    assert_eq!(
        merged.keys.get("database.url").expect("the key").fields["required"],
        json!(true)
    );
}

#[test]
fn a_differing_field_outside_required_is_a_hard_error() {
    // The catch-all: naming the fields that matter means the next one a producer adds falls through
    // the gap in silence, so everything but `required` has to agree.
    let mut right = fixture("worker");
    key(&mut right, "database.url")["docs"] = json!("Something else entirely.");
    let error =
        merge(vec![("api", fixture("api")), ("worker", right)]).expect_err("`docs` must agree");
    assert!(error.to_string().contains("`docs`"), "{error}");
}

#[test]
fn an_empty_union_is_refused() {
    assert!(merge(Vec::new()).is_err());
}

// ---------------------------------------------------------------------------------------------
// The external surface
// ---------------------------------------------------------------------------------------------

#[test]
fn ignore_patterns_are_unioned_and_sorted() {
    assert_eq!(
        union(&["api", "worker"]).ignore,
        ["HOSTNAME", "KUBERNETES_*"]
    );
}

#[test]
fn the_strictest_unknown_policy_wins_whatever_the_order() {
    // `api` rejects and `worker` warns: a variable any reader refuses is one the pod cannot carry.
    assert_eq!(union(&["api", "worker"]).unknown, "reject");
    assert_eq!(union(&["worker", "api"]).unknown, "reject");
}

#[test]
fn external_variables_are_kept() {
    assert!(union(&["api", "worker"]).external_env.contains("PORT"));
}

#[test]
fn two_descriptions_of_one_external_variable_is_a_hard_error() {
    let left = fixture("api");
    let mut right = fixture("worker");
    let mut theirs = left["external"]["env"][0].clone();
    theirs["ty"] = json!("String");
    theirs["constraint"] = json!({"type": "string"});
    right["external"]["env"] = json!([theirs]);
    let error =
        merge(vec![("api", left), ("worker", right)]).expect_err("one variable, two descriptions");
    assert!(error.to_string().contains("PORT"), "{error}");
}

// ---------------------------------------------------------------------------------------------
// The schema
// ---------------------------------------------------------------------------------------------

#[test]
fn properties_from_every_contract_are_merged() {
    let merged = union(&["api", "worker"]).json_schema;
    let mut names: Vec<&str> = merged["properties"]
        .as_object()
        .expect("the merged schema has properties")
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["auth", "database", "github", "log", "tuning", "worker"]
    );
}

#[test]
fn every_level_is_closed_after_the_union() {
    let merged = union(&["api", "worker"]).json_schema;
    assert_eq!(merged["additionalProperties"], json!(false));
    for name in ["auth", "database", "worker"] {
        assert_eq!(
            merged["properties"][name]["additionalProperties"],
            json!(false),
            "{name} is open"
        );
    }
}

#[test]
fn a_single_contract_is_closed_at_every_level_too() {
    // The first contract merged takes the same path as every one after it: a document read by one
    // image must come out as closed as one read by eight, or a producer that left a level open
    // would leave the gate open there.
    let mut left = fixture("api");
    left["json_schema"]["properties"]["auth"]["additionalProperties"] = json!(true);
    let merged = merge(vec![("api", left)])
        .expect("one contract merges")
        .json_schema;
    assert_eq!(
        merged["properties"]["auth"]["additionalProperties"],
        json!(false)
    );
}

#[test]
fn required_is_unioned_per_object() {
    let mut right = fixture("worker");
    right["json_schema"]["properties"]["database"]["required"] = json!(["url", "pool_size"]);
    right["json_schema"]["properties"]["database"]["properties"]["pool_size"] =
        json!({"type": "integer"});
    let merged = merge(vec![("api", fixture("api")), ("worker", right)])
        .expect("required unions per object")
        .json_schema;
    assert_eq!(
        merged["properties"]["database"]["required"],
        json!(["pool_size", "url"])
    );
}

#[test]
fn two_types_for_one_path_is_a_hard_error() {
    let error = merge(vec![
        ("api", fixture("api")),
        ("conflicting", schema_only(fixture("conflicting"))),
    ])
    .expect_err("one path cannot be two types");
    assert!(
        error.to_string().contains("$.auth.session_ttl"),
        "the message names the position: {error}"
    );
}

#[test]
fn a_differing_bound_is_a_hard_error() {
    let mut right = schema_only(fixture("worker"));
    right["json_schema"]["properties"]["auth"] = json!({
        "type": "object",
        "properties": {"session_ttl": {"type": "integer", "minimum": 60, "default": 3600}},
    });
    let error = merge(vec![("api", fixture("api")), ("worker", right)])
        .expect_err("one contract accepts a value the other refuses");
    assert!(error.to_string().contains("minimum"), "{error}");
}

#[test]
fn a_differing_description_is_a_hard_error_too() {
    // The catch-all covers annotations, which is the rule as specified: two binaries that document
    // one key differently have either drifted or are describing two different things.
    let mut left = fixture("api");
    left["json_schema"]["properties"]["auth"]["properties"]["session_ttl"]["description"] =
        json!("a");
    let mut right = schema_only(fixture("worker"));
    right["json_schema"]["properties"]["auth"] = json!({
        "type": "object",
        "properties": {"session_ttl": {
            "type": "integer", "minimum": 0, "default": 3600, "description": "b",
        }},
    });
    let error = merge(vec![("api", left), ("worker", right)]).expect_err("descriptions must agree");
    assert!(error.to_string().contains("description"), "{error}");
}

#[test]
fn a_differing_root_title_is_a_hard_error() {
    // Recorded deliberately. Every generated contract carries `title: "<app> configuration"`, so
    // under the catch-all no two contracts of one *document* can be unioned until a producer omits
    // it or the rule exempts it. The first chart to declare a document with two readers is where
    // this bites, and it should bite loudly rather than merge one title over the other.
    let mut right = schema_only(fixture("worker"));
    right["json_schema"]["title"] = json!("worker configuration");
    let error =
        merge(vec![("api", fixture("api")), ("worker", right)]).expect_err("titles must agree");
    assert!(error.to_string().contains("title"), "{error}");
}

#[test]
fn an_element_schema_survives_the_merge_rather_than_being_dropped() {
    // The one the fixture set exists for at `schema_version: 2`: a map's `additionalProperties` is
    // an element schema and not the open/closed flag, and an earlier rule that treated the two the
    // same silently discarded the fact the union exists to preserve.
    let merged = union(&["deep"]).json_schema;
    let element = &merged["properties"]["webhook"]["properties"]["paths"]["additionalProperties"];
    assert!(element.is_object(), "{element}");
    assert!(
        element.get("additionalProperties").is_none(),
        "an element schema is the producer's to close: {element}"
    );
}

// ---------------------------------------------------------------------------------------------
// Remote references
// ---------------------------------------------------------------------------------------------

#[test]
fn a_remote_reference_is_reported() {
    // Constructed rather than stored. An offline gate that resolves a remote reference has silently
    // become a networked one, and the fixture for that is two lines — a file would be a file whose
    // whole content is one keyword.
    let offenders = local_refs_only(&json!({
        "properties": {"thing": {"$ref": "https://example.invalid/schema.json"}}
    }));
    assert_eq!(offenders.len(), 1, "{offenders:?}");
    assert!(offenders[0].contains("example.invalid"), "{offenders:?}");
}

#[test]
fn a_local_reference_is_allowed() {
    assert!(local_refs_only(&json!({"$ref": "#/definitions/thing"})).is_empty());
}

#[test]
fn the_real_fixtures_carry_none() {
    assert!(local_refs_only(&union(&["api", "worker"]).json_schema).is_empty());
}

// ---------------------------------------------------------------------------------------------
// The envelope
//
// The implementation this was ported from gated on the envelope in one hand-written function.
// Here the same three refusals belong to three places, each of which is the right one: the reader
// refuses a version it cannot read, the published meta-schema says what a section must be, and the
// value checks refuse a form they do not implement.
// ---------------------------------------------------------------------------------------------

#[test]
fn an_unrecognised_envelope_version_is_refused_by_name() {
    let mut document = fixture("api");
    document["terrace_contract"] = json!(2);
    let error = Contract::from_json(&document.to_string())
        .expect_err("an envelope this build cannot read is refused");
    assert!(error.to_string().contains("terrace_contract"), "{error}");
    assert!(error.to_string().contains('2'), "{error}");
}

#[test]
fn a_missing_section_is_refused_by_the_published_schema() {
    let mut document = fixture("api");
    document
        .as_object_mut()
        .expect("an object")
        .remove("external");
    let failures =
        terrace_contract::validate::validate(&document.to_string()).expect("the document is JSON");
    assert!(
        failures.iter().any(|failure| failure.contains("external")),
        "{failures:?}"
    );
}

#[test]
fn an_unknown_policy_is_refused_by_the_published_schema() {
    let mut document = fixture("api");
    document["external"]["unknown"] = json!("ignore-everything");
    let failures =
        terrace_contract::validate::validate(&document.to_string()).expect("the document is JSON");
    assert!(!failures.is_empty(), "an unknown policy must be refused");
}

#[test]
fn a_text_form_this_build_cannot_read_is_refused_rather_than_downgraded() {
    // A hard error and not a silent skip. Under-checking a value while reporting success is the
    // failure this whole half exists to remove, and the envelope version is the lever a producer
    // has for saying the vocabulary changed.
    let mut document = fixture("api");
    key(&mut document, "database.url")["text_form"] = json!("duration");
    let merged = merge(vec![("api", document)]).expect("the union does not read forms");
    let held = merged.keys.get("database.url").expect("the key");
    let error = Entry(&held.fields)
        .text_form()
        .expect_err("a form this build has not implemented is refused");
    assert!(error.to_string().contains("duration"), "{error}");
}
