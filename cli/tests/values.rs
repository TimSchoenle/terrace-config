//! The classification order and the two-step value check.
//!
//! Both are normative rather than convenient. Two consumers running the classification steps in
//! different orders disagree about whether a deployment is valid, and a consumer that runs only one
//! of the two value checks either leaves every bound in the document decorative or refuses a correct
//! deployment. Neither failure is visible from the outside — the gate goes green either way — so
//! these are the tests that say the implementation is the specified one.
//!
//! # Where the split moved
//!
//! The implementation this was ported from ran a grammar check inside step 1: "is this text an
//! integer" sat beside "does this text match the published pattern". Only the second is portable.
//! Whether text *reads* as a value is the loader's question — figment wants a TOML literal where a
//! relaxed binder splits on commas — so it moved into step 2, behind the registry, and step 1 is
//! now exactly what the producer published about its own reads and nothing else.

#![cfg(feature = "helm")]

use std::path::{Path, PathBuf};

use serde_json::{Value as Json, json};
use terrace_contract::classify::{Kind, classify, matches_ignore};
use terrace_contract::union::{Merged, Union, suggest, union_contracts};
use terrace_contract::value::{Range, assert_value, form, range};

const FIGMENT: &str = "figment";

fn fixture(name: &str) -> Json {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()));
    serde_json::from_str(&text).expect("a fixture is JSON")
}

fn union(names: &[&str]) -> Union {
    let loaded: Vec<(String, Json)> = names
        .iter()
        .map(|name| ((*name).to_owned(), fixture(name)))
        .collect();
    union_contracts(&loaded).expect("the fixtures merge")
}

fn key<'a>(merged: &'a Union, path: &str) -> &'a Merged {
    merged
        .keys
        .get(path)
        .unwrap_or_else(|| panic!("the fixtures declare {path}"))
}

/// Both steps, in the order a gate runs them: the form, and the range only if the form passed.
///
/// One value produces one line, which is why the range step is not reached when the form failed —
/// it would be reporting on a read that never happened.
fn check(entry: &Merged, text: &str) -> Option<String> {
    if let Some(failure) = form(entry.entry(), text).expect("the form is known") {
        return Some(failure);
    }
    match range(entry.entry(), FIGMENT, text).expect("the form is known") {
        Range::Wrong(failure) => Some(failure),
        Range::Ok | Range::NotChecked(_) => None,
    }
}

// ---------------------------------------------------------------------------------------------
// The classification order
// ---------------------------------------------------------------------------------------------

#[test]
fn step_1_a_loader_variable() {
    let merged = union(&["api", "worker"]);
    assert_eq!(classify(&merged, "FIXTURE_CONFIG").kind, Kind::Loader);
    assert_eq!(classify(&merged, "FIXTURE_SECRETS_DIR").kind, Kind::Loader);
}

#[test]
fn step_2_a_key_supplied_by_the_environment() {
    let merged = union(&["api", "worker"]);
    let decision = classify(&merged, "FIXTURE_AUTH__SESSION_TTL");
    assert_eq!(decision.kind, Kind::KeyEnv);
    assert_eq!(
        decision.entry.map(|entry| entry.name.as_str()),
        Some("auth.session_ttl")
    );
}

#[test]
fn step_3_a_key_supplied_by_indirection() {
    let merged = union(&["api", "worker"]);
    let decision = classify(&merged, "FIXTURE_AUTH__SESSION_TTL_FILE");
    assert_eq!(decision.kind, Kind::KeyEnvFile);
    assert_eq!(
        decision.entry.map(|entry| entry.name.as_str()),
        Some("auth.session_ttl")
    );
}

#[test]
fn step_4_a_prefixed_variable_spelling_nothing_is_rejected() {
    let merged = union(&["api", "worker"]);
    assert_eq!(classify(&merged, "FIXTURE_NOPE").kind, Kind::Prefixed);
}

#[test]
fn step_5_a_declared_external_variable() {
    assert_eq!(
        classify(&union(&["api", "worker"]), "PORT").kind,
        Kind::External
    );
}

#[test]
fn step_6_an_ignored_variable() {
    let merged = union(&["api", "worker"]);
    assert_eq!(
        classify(&merged, "KUBERNETES_SERVICE_HOST").kind,
        Kind::Ignored
    );
    assert_eq!(classify(&merged, "HOSTNAME").kind, Kind::Ignored);
}

#[test]
fn step_7_everything_else() {
    assert_eq!(
        classify(&union(&["api", "worker"]), "PATH").kind,
        Kind::Unknown
    );
}

#[test]
fn step_4_outranks_both_external_lists() {
    // The load-bearing part. A producer refuses to build a contract whose `external.env` or whose
    // `ignore` reaches into the loader's own namespace; this is the consumer-side half of the same
    // guarantee — even if one arrived, step 4 rejects the variable before either list is consulted.
    let mut document = fixture("api");
    document["external"]["env"] = json!([{"name": "FIXTURE_AUTH__NOPE", "text_form": "text"}]);
    document["external"]["ignore"] = json!(["FIXTURE_*"]);
    let merged = union_contracts(&[("api".to_owned(), document)]).expect("it merges");

    assert_eq!(
        classify(&merged, "FIXTURE_AUTH__NOPE").kind,
        Kind::Prefixed,
        "an external list must not be able to exempt a variable inside the namespace"
    );
}

// ---------------------------------------------------------------------------------------------
// The ignore pattern language
// ---------------------------------------------------------------------------------------------

#[test]
fn a_trailing_star_is_a_prefix_match() {
    assert!(matches_ignore("KUBERNETES_*", "KUBERNETES_SERVICE_HOST"));
    assert!(matches_ignore("KUBERNETES_*", "KUBERNETES_"));
}

#[test]
fn anything_else_is_exact() {
    assert!(matches_ignore("HOSTNAME", "HOSTNAME"));
    assert!(!matches_ignore("HOSTNAME", "HOSTNAME_2"));
}

#[test]
fn a_star_in_the_middle_is_not_a_wildcard() {
    assert!(!matches_ignore("A*B", "AXB"));
}

#[test]
fn it_is_not_a_glob() {
    assert!(!matches_ignore("PORT?", "PORTS"));
    assert!(!matches_ignore("[AB]", "A"));
}

// ---------------------------------------------------------------------------------------------
// The two-step check
// ---------------------------------------------------------------------------------------------

#[test]
fn a_well_formed_integer_passes_both_steps() {
    let merged = union(&["api", "worker"]);
    assert_eq!(check(key(&merged, "worker.concurrency"), "8080"), None);
}

#[test]
fn the_form_step_rejects_text_that_is_not_an_integer() {
    let merged = union(&["api", "worker"]);
    let held = key(&merged, "worker.concurrency");
    assert!(
        form(held.entry(), "http")
            .expect("the form is known")
            .is_some()
    );
}

#[test]
fn the_range_step_is_the_only_one_a_bound_is_reachable_from() {
    // 99999 is a perfectly well-formed integer; only the bound catches it not fitting a u16. This
    // is the check a form-only implementation silently omits, and the reason skipping step 2 leaves
    // every bound in the document decorative.
    let merged = union(&["api", "worker"]);
    let held = key(&merged, "worker.concurrency");
    assert_eq!(
        form(held.entry(), "99999").expect("the form is known"),
        None
    );
    let failure = check(held, "99999").expect("99999 does not fit a u16");
    assert!(failure.contains("65535"), "{failure}");
}

#[test]
fn the_range_step_does_not_reject_a_correct_integer_as_text() {
    // Applying `constraint` to the raw text would fail `"0"` against `{"type": "integer"}` and
    // refuse a correct deployment. The text is *read* first, which is the whole of step 2.
    let merged = union(&["api", "worker"]);
    assert_eq!(check(key(&merged, "auth.session_ttl"), "0"), None);
}

#[test]
fn the_loaders_integer_grammar() {
    let merged = union(&["api", "worker"]);
    let held = key(&merged, "auth.session_ttl");
    for accepted in ["0", "42", "007", "+5", " 7 "] {
        assert_eq!(check(held, accepted), None, "{accepted} loads");
    }
    for refused in ["1_000", "0x1F", "0b1", "1e3", ""] {
        assert!(check(held, refused).is_some(), "{refused} does not load");
    }
}

#[test]
fn a_boolean_takes_only_the_two_spellings() {
    let merged = union(&["api", "worker"]);
    let held = key(&merged, "worker.debug");
    assert_eq!(check(held, "true"), None);
    assert_eq!(check(held, "false"), None);
    for refused in ["TRUE", "1", "yes", "True"] {
        assert!(check(held, refused).is_some(), "{refused} is not a boolean");
    }
}

#[test]
fn a_choice_takes_only_the_listed_spellings() {
    let merged = union(&["api", "worker"]);
    let held = key(&merged, "log.level");
    assert_eq!(check(held, "debug"), None);
    assert!(check(held, "verbose").is_some());
}

#[test]
fn text_is_unconstrained_and_has_no_range_step() {
    let merged = union(&["api", "worker"]);
    assert_eq!(check(key(&merged, "database.url"), "anything at all"), None);
}

#[test]
fn unknown_skips_both_steps_rather_than_guessing() {
    // A gap, not an answer, and deliberately distinct from `text` — which means "any text is
    // correct" rather than "nothing could be determined".
    let merged = union(&["api", "worker"]);
    assert_eq!(check(key(&merged, "tuning.ratio"), "whatever"), None);
}

#[test]
fn a_structured_value_must_carry_its_brackets() {
    // `a,b` reads like a list, is not one, and is refused by the loader at boot.
    let merged = union(&["api", "worker"]);
    let failure = check(key(&merged, "github.repos"), "a,b").expect("a list carries its brackets");
    assert!(failure.contains("brackets"), "{failure}");
}

#[test]
fn a_structured_value_with_brackets_reads() {
    let merged = union(&["api", "worker"]);
    let held = key(&merged, "github.repos");
    assert_eq!(
        form(held.entry(), r#"["a", "b"]"#).expect("the form is known"),
        None
    );
    assert_eq!(
        form(held.entry(), "{ a = 1 }").expect("the form is known"),
        None
    );
    assert_eq!(check(held, r#"["a", "b"]"#), None);
}

#[test]
fn a_structured_value_is_held_to_its_constraint_once_it_has_been_read() {
    // Wider than the implementation this was ported from, which never reached this: its read
    // returned the text unparsed, so `constraint` was unreachable for every container-typed key.
    // `FORMAT.md` step 2 exempts nothing — "read the text according to `text_form`, then check the
    // result against `constraint`" — and `github.repos` is a sequence, so an inline table is a
    // value the loader will refuse at boot.
    let merged = union(&["api", "worker"]);
    let failure =
        check(key(&merged, "github.repos"), "{ a = 1 }").expect("an inline table is not a list");
    assert!(failure.contains("expected"), "{failure}");
}

#[test]
fn the_form_is_read_from_text_form_and_never_inferred() {
    // A key whose constraint looks integral but whose form says `text` must not be read as an
    // integer. The inference "a pattern means integer" was right for two shapes and wrong for
    // three, and this is the case that separates them.
    let merged = union(&["api", "worker"]);
    let mut held = key(&merged, "database.url").clone();
    held.fields.insert(
        "constraint".to_owned(),
        json!({"type": "integer", "maximum": 10}),
    );
    assert_eq!(check(&held, "99999"), None);
}

#[test]
fn an_external_variable_is_checked_the_same_two_ways() {
    let merged = union(&["api", "worker"]);
    let held = merged.external_env.get("PORT").expect("PORT is declared");
    assert!(
        form(held.entry(), "http")
            .expect("the form is known")
            .is_some()
    );
    assert_eq!(
        form(held.entry(), "99999").expect("the form is known"),
        None
    );
    assert!(check(held, "99999").is_some());
}

// ---------------------------------------------------------------------------------------------
// What a file can supply
// ---------------------------------------------------------------------------------------------

#[test]
fn only_a_text_key_can_be_supplied_by_a_file() {
    // A file layer delivers a string with no parse, and no loader here coerces one into a number, a
    // boolean or a TOML literal. `unknown` is refused too, and deliberately: nothing is known about
    // how the loader reads it, so nothing says a raw string will do. A false report is one line in
    // review; a missing one is a credential silently unread.
    let merged = union(&["api", "worker"]);
    let supplyable = |path: &str| {
        key(&merged, path)
            .entry()
            .file_supplyable()
            .expect("the form is known")
    };
    assert!(supplyable("database.url"));
    for path in [
        "auth.session_ttl",
        "worker.debug",
        "github.repos",
        "log.level",
        "tuning.ratio",
    ] {
        assert!(!supplyable(path), "{path} cannot be supplied by a file");
    }
}

// ---------------------------------------------------------------------------------------------
// The constraint vocabulary, and the suggestion
// ---------------------------------------------------------------------------------------------

#[test]
fn an_unimplemented_keyword_is_an_error_rather_than_a_silent_skip() {
    let error = assert_value(
        &json!({"type": "string", "contentEncoding": "base64"}),
        &json!("x"),
        "",
    )
    .expect_err("under-checking a value while reporting success is the failure to avoid");
    assert!(error.to_string().contains("contentEncoding"), "{error}");
}

#[test]
fn annotations_are_not_assertions() {
    assert_eq!(
        assert_value(
            &json!({"type": "string", "description": "x"}),
            &json!("y"),
            ""
        )
        .expect("the vocabulary is implemented"),
        None
    );
}

#[test]
fn a_boolean_is_not_an_integer() {
    assert!(
        assert_value(&json!({"type": "integer"}), &json!(true), "")
            .expect("the vocabulary is implemented")
            .is_some()
    );
}

#[test]
fn a_near_miss_is_offered() {
    assert!(
        suggest("isr.ttl_sec", ["isr.ttl_secs", "isr.cache_dir"]).contains("isr.ttl_secs"),
        "a rename should cost one line rather than a search"
    );
}

#[test]
fn nothing_close_offers_nothing() {
    assert_eq!(suggest("completely.different", ["isr.ttl_secs"]), "");
}

#[test]
fn the_name_itself_is_never_offered() {
    assert_eq!(suggest("isr.ttl_secs", ["isr.ttl_secs"]), "");
}
