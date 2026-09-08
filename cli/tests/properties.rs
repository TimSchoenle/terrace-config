//! The rules, tested by construction rather than against the corpus.
//!
//! [`corpus.rs`](corpus.rs) pins what three real documents render to, which is the strongest check
//! available for a renderer and says nothing at all about the cases those three documents do not
//! contain. Every case here is one of those: a refusal no conformance document may carry, a
//! spelling only a foreign producer emits, a shape only a mutation reaches.
//!
//! Written against hand-built documents rather than against mutated ones, because a test whose
//! input a reader cannot see is a test nobody maintains. `fuzz/` is where the mutated ones live.

use serde_json::{Value as Json, json};
use terrace_contract::conform::{self, Tier};
use terrace_contract::render::{self, Format, Options};
use terrace_contract::{Contract, TextForm, Unreachable, validate};

/// A document with the given schema half, and everything else at its least interesting.
///
/// Built as JSON and read back rather than constructed as a `Contract`, because that is the path a
/// real document takes and because the reader is half of what these tests are about.
fn document(schema: &Json, external: &Json) -> Contract {
    let text = serde_json::to_string(&json!({
        "terrace_contract": 1,
        "producer": {"name": "test", "version": "0", "loader": "figment"},
        "app": {"name": "test"},
        "schema": schema,
        "json_schema": {},
        "external": external,
    }))
    .expect("the fixture serialises");
    Contract::from_json(&text).expect("the fixture is a readable document")
}

fn dialect() -> Json {
    json!({"prefix": "T_", "nesting_separator": "__", "indirection_suffix": "_FILE"})
}

/// A key with every required field, overridable through `extra`.
fn key(path: &str, extra: Json) -> Json {
    let mut base = json!({
        "path": path,
        "env": format!("T_{}", path.replace('.', "__").to_uppercase()),
        "env_file": format!("T_{}_FILE", path.replace('.', "__").to_uppercase()),
        "secrets_file": path.replace('.', "__"),
        "docs": "",
        "ty": "String",
        "values": [],
        "constraint": {"type": "string"},
        "text_form": "text",
        "aliases": [],
        "env_aliases": [],
        "env_file_aliases": [],
        "secrets_file_aliases": [],
        "default": null,
        "default_value": null,
        "note": null,
        "required": false,
        "secret": false,
        "reserved": false
    });
    if let (Json::Object(base), Json::Object(extra)) = (&mut base, extra) {
        for (name, value) in extra {
            base.insert(name, value);
        }
    }
    base
}

fn schema(keys: &Json) -> Json {
    json!({
        "schema_version": 2,
        "dialect": dialect(),
        "loader": [{"env": "T_CONFIG", "role": "config", "docs": "The file.", "default": "config.toml"}],
        "keys": keys,
    })
}

/// The default a secret is given when a test needs one to carry.
///
/// Deliberately inert. The rule under test is "a secret carries a default at all", never that the
/// default is plausible — and a fixture that reads like a real credential is what a secret scanner
/// is built to notice, in this repository and in every fork of it.
const PLACEHOLDER_DEFAULT: &str = "not-a-credential";

fn no_external() -> Json {
    json!({"env": [], "ignore": [], "unknown": "reject"})
}

fn rules(contract: &Contract, tier: Tier) -> Vec<&'static str> {
    conform::conform(contract, tier)
        .iter()
        .map(|violation| violation.rule)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The refusals, one test each. FORMAT.md numbers them; so do these.
// ---------------------------------------------------------------------------------------------

#[test]
fn refusal_1_an_external_variable_inside_the_namespace() {
    let contract = document(
        &schema(&json!([])),
        &json!({
            "env": [{"name": "T_SNEAKY", "docs": "", "values": [], "text_form": "text",
                     "required": false, "secret": false}],
            "ignore": [], "unknown": "reject"
        }),
    );
    assert!(rules(&contract, Tier::Document).contains(&"refusal 1"));
}

#[test]
fn refusal_2_a_pattern_that_subsumes_the_prefix_without_carrying_it() {
    // The case FORMAT.md calls out by name. `T*` reads as a pattern about some external `T` and
    // disables the whole gate; an exact `T` is fine and must not be reported.
    let wide = document(
        &schema(&json!([])),
        &json!({"env": [], "ignore": ["T*"], "unknown": "reject"}),
    );
    assert!(rules(&wide, Tier::Document).contains(&"refusal 2"));

    let exact = document(
        &schema(&json!([])),
        &json!({"env": [], "ignore": ["T"], "unknown": "reject"}),
    );
    assert!(
        !rules(&exact, Tier::Document).contains(&"refusal 2"),
        "an exact name that is a prefix of the namespace is not a pattern reaching into it"
    );
}

#[test]
fn refusal_3_an_external_variable_the_loader_reads() {
    let contract = document(
        &schema(&json!([])),
        &json!({
            "env": [{"name": "T_CONFIG", "docs": "", "values": [], "text_form": "text",
                     "required": false, "secret": false}],
            "ignore": [], "unknown": "reject"
        }),
    );
    let found = rules(&contract, Tier::Document);
    assert!(found.contains(&"refusal 3"), "{found:?}");
}

#[test]
fn refusal_4_a_pattern_covering_a_loader_variable_outside_the_prefix() {
    // The prefix is not the whole namespace: the config variable takes an arbitrary name, so a
    // pattern can cover it without ever touching the prefix.
    let mut schema = schema(&json!([]));
    schema["loader"] = json!([
        {"env": "CREDENTIALS_DIR", "role": "secrets_dir", "docs": "", "default": null}
    ]);
    let contract = document(
        &schema,
        &json!({"env": [], "ignore": ["CREDENTIALS_*"], "unknown": "reject"}),
    );
    assert!(rules(&contract, Tier::Document).contains(&"refusal 4"));
}

#[test]
fn refusal_5_an_external_variable_declared_twice() {
    let var = json!({"name": "PORT", "docs": "", "values": [], "text_form": "text",
                     "required": false, "secret": false});
    let contract = document(
        &schema(&json!([])),
        &json!({"env": [var, var], "ignore": [], "unknown": "reject"}),
    );
    assert!(rules(&contract, Tier::Document).contains(&"refusal 5"));
}

#[test]
fn refusal_6_a_secret_carrying_a_default_in_either_half() {
    let on_a_key = document(
        &schema(&json!([key(
            "token",
            json!({"secret": true, "default_value": PLACEHOLDER_DEFAULT})
        )])),
        &no_external(),
    );
    assert!(rules(&on_a_key, Tier::Document).contains(&"refusal 6"));

    let on_a_variable = document(
        &schema(&json!([])),
        &json!({
            "env": [{"name": "TOKEN", "docs": "", "values": [], "text_form": "text",
                     "default": PLACEHOLDER_DEFAULT, "required": false, "secret": true}],
            "ignore": [], "unknown": "reject"
        }),
    );
    assert!(rules(&on_a_variable, Tier::Document).contains(&"refusal 6"));
}

#[test]
fn refusal_7_an_empty_prefix() {
    let mut half = schema(&json!([]));
    half["dialect"]["prefix"] = json!("");
    let contract = document(&half, &no_external());
    assert!(rules(&contract, Tier::Document).contains(&"refusal 7"));
}

#[test]
fn refusal_8_a_key_reachable_only_through_another_keys_indirection() {
    let contract = document(
        &schema(&json!([key(
            "token",
            json!({"env": null, "unreachable": "indirection"})
        )])),
        &no_external(),
    );
    assert!(rules(&contract, Tier::Document).contains(&"refusal 8"));
}

#[test]
fn a_document_with_nothing_wrong_reports_nothing() {
    let contract = document(
        &schema(&json!([key("dist_dir", json!({}))])),
        &no_external(),
    );
    let found = conform::conform(&contract, Tier::Dialect);
    assert!(found.is_empty(), "{found:?}");
}

// ---------------------------------------------------------------------------------------------
// Tier 2 — the tier a relaxed binder is expected to fail
// ---------------------------------------------------------------------------------------------

#[test]
fn tier_2_accepts_a_derived_spelling_and_refuses_one_that_is_merely_plausible() {
    let derived = document(
        &schema(&json!([key("github.token", json!({}))])),
        &no_external(),
    );
    assert!(conform::conform(&derived, Tier::Dialect).is_empty());

    // What Spring's relaxed binding would emit: one separator where the dialect says two.
    let relaxed = document(
        &schema(&json!([key(
            "github.token",
            json!({"env": "T_GITHUB_TOKEN", "env_file": "T_GITHUB_TOKEN_FILE"})
        )])),
        &no_external(),
    );
    assert!(rules(&relaxed, Tier::Dialect).contains(&"tier 2"));
    assert!(
        conform::conform(&relaxed, Tier::Document).is_empty(),
        "a tier 1 producer with its own naming is still a valid tier 1 producer"
    );
}

#[test]
fn tier_2_refuses_an_unspelled_key_that_does_not_say_why() {
    // A bare `null` is read downstream as "skip this key", which is right for `unnameable` and
    // wrong for `indirection`. A key that says neither leaves the consumer guessing.
    let silent = document(
        &schema(&json!([key(
            "distDir",
            json!({"env": null, "env_file": null, "unreachable": null})
        )])),
        &no_external(),
    );
    assert!(rules(&silent, Tier::Dialect).contains(&"tier 2"));

    let explained = document(
        &schema(&json!([key(
            "distDir",
            json!({"env": null, "env_file": null, "unreachable": "unnameable"})
        )])),
        &no_external(),
    );
    assert!(conform::conform(&explained, Tier::Dialect).is_empty());
}

#[test]
fn tier_2_refuses_an_indirection_spelling_the_suffix_does_not_derive() {
    let contract = document(
        &schema(&json!([key("token", json!({"env_file": "T_TOKEN_PATH"}))])),
        &no_external(),
    );
    assert!(rules(&contract, Tier::Dialect).contains(&"tier 2"));
}

// ---------------------------------------------------------------------------------------------
// The renderings, on shapes the corpus does not contain
// ---------------------------------------------------------------------------------------------

#[test]
fn a_foreign_type_name_changes_nothing_about_the_placeholder() {
    // The property that makes this renderer language-agnostic. A Java producer's `ty` is a name no
    // Rust vocabulary contains, and the placeholder still has to come out the right shape.
    let java = document(
        &schema(&json!([key(
            "retries",
            json!({"ty": "java.lang.Integer", "constraint": {"type": "integer"}})
        )])),
        &no_external(),
    );
    let rendered =
        render::render(&java, Format::Toml, &Options::default()).expect("the example renders");
    assert!(
        rendered.contains("retries = 0"),
        "an integer placeholder is a bare `0` whatever the producer calls the type:\n{rendered}"
    );

    let rust = document(
        &schema(&json!([key(
            "retries",
            json!({"ty": "u32", "constraint": {"type": "integer"}})
        )])),
        &no_external(),
    );
    // Everything *except* the `Type:` line, which is the one place `ty` legitimately appears: it is
    // shown to an operator as the producer's own name for the type, and never switched on. That is
    // the whole distinction — a renderer may print `ty`, and must not branch on it.
    let from_rust = render::render(&rust, Format::Toml, &Options::default()).expect("renders");
    let without_type = |file: &str| {
        file.lines()
            .filter(|line| !line.starts_with("# Type:"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        without_type(&from_rust),
        without_type(&rendered),
        "two producers describing one key differ only in what they call its type"
    );
    assert!(from_rust.contains("# Type: u32"));
    assert!(rendered.contains("# Type: java.lang.Integer"));
}

#[test]
fn a_key_with_no_constraint_gets_a_quoted_placeholder() {
    // A domain newtype, or a type the producer did not recognise. Inventing a shape would write an
    // example that fails to parse for a key the image accepts.
    let contract = document(
        &schema(&json!([key(
            "opaque",
            json!({"ty": "DomainNewtype", "constraint": null})
        )])),
        &no_external(),
    );
    let rendered = render::render(&contract, Format::Toml, &Options::default()).expect("renders");
    assert!(rendered.contains(r#"opaque = "<value>""#), "{rendered}");
    toml::from_str::<toml::Value>(&rendered).expect("the example parses");
}

#[test]
fn a_hostile_key_name_does_not_break_the_table_or_the_file() {
    let contract = document(
        &schema(&json!([key(
            "a|b",
            json!({"docs": "One | two\n\nAnd a second paragraph."})
        )])),
        &no_external(),
    );

    let table =
        render::render(&contract, Format::MarkdownKeys, &Options::default()).expect("renders");
    for line in table.lines() {
        assert_eq!(
            line.matches('|').count() - line.matches(r"\|").count(),
            7,
            "an unescaped separator adds a column:\n{table}"
        );
    }
    assert!(
        table.contains("One \\| two"),
        "the summary is escaped and the second paragraph is not carried into the cell:\n{table}"
    );
    assert!(!table.contains("second paragraph"), "{table}");

    let file = render::render(&contract, Format::Toml, &Options::default()).expect("renders");
    toml::from_str::<toml::Value>(&file)
        .expect("a hostile key name still yields a file that loads");
}

#[test]
fn a_default_toml_cannot_hold_is_left_out_rather_than_written_unparseably() {
    let contract = document(
        &schema(&json!([key(
            "big",
            json!({
                "ty": "u64",
                "constraint": {"type": "integer"},
                "default_value": u64::MAX,
                "default": "18446744073709551615"
            })
        )])),
        &no_external(),
    );
    let rendered = render::render(&contract, Format::Toml, &Options::default()).expect("renders");
    assert!(
        rendered.contains("big = 0"),
        "an integer past TOML's signed range falls back to the shape:\n{rendered}"
    );
    toml::from_str::<toml::Value>(&rendered).expect("the example parses");
}

#[test]
fn a_reserved_key_is_left_out_of_the_json_schema() {
    // No file supplies a reserved key, so a schema describing one describes a key the document it
    // validates cannot carry.
    let contract = document(
        &schema(&json!([
            key("profile", json!({"reserved": true})),
            key("dist_dir", json!({}))
        ])),
        &no_external(),
    );
    let rendered: Json = serde_json::from_str(
        &render::render(&contract, Format::JsonSchema, &Options::default()).expect("renders"),
    )
    .expect("the rendering is JSON");

    let properties = &rendered["properties"];
    assert!(properties.get("dist_dir").is_some());
    assert!(
        properties.get("profile").is_none(),
        "a reserved key is not a property of the file:\n{rendered:#}"
    );
}

#[test]
fn an_alias_is_a_property_of_its_own_so_a_closed_schema_does_not_refuse_it() {
    let contract = document(
        &schema(&json!([key("dist_dir", json!({"aliases": ["distDir"]}))])),
        &no_external(),
    );
    let rendered: Json = serde_json::from_str(
        &render::render(&contract, Format::JsonSchema, &Options::default()).expect("renders"),
    )
    .expect("the rendering is JSON");

    assert!(rendered["properties"].get("distDir").is_some());
    assert_eq!(rendered["additionalProperties"], json!(false));
}

// ---------------------------------------------------------------------------------------------
// The reader
// ---------------------------------------------------------------------------------------------

#[test]
fn an_envelope_this_build_does_not_read_is_refused_before_anything_else_is() {
    // Deliberately also malformed below the envelope: the version gate must fire first, or the
    // message a reader gets is about a field they were never going to reach.
    let error = Contract::from_json(r#"{"terrace_contract": 7, "schema": 5}"#)
        .expect_err("envelope 7 is not readable");
    let message = error.to_string();
    assert!(message.contains('7'), "{message}");
    assert!(!message.contains("schema"), "{message}");
}

#[test]
fn a_schema_version_ahead_is_read_and_reported_rather_than_refused() {
    let mut half = schema(&json!([]));
    half["schema_version"] = json!(99);
    let contract = document(&half, &no_external());
    assert_eq!(contract.schema_version_ahead(), Some(99));
}

#[test]
fn an_unknown_vocabulary_value_does_not_fail_a_document_this_build_can_read() {
    let contract = document(
        &schema(&json!([key("x", json!({"text_form": "duration"}))])),
        &json!({"env": [], "ignore": [], "unknown": "audit"}),
    );
    assert_eq!(contract.schema.keys[0].text_form, TextForm::Unknown);
    assert_eq!(
        contract.schema.keys[0].unreachable, None,
        "an absent field stays absent"
    );
}

#[test]
fn a_document_this_build_writes_is_one_it_reads_back_unchanged() {
    let contract = document(
        &schema(&json!([
            key("a", json!({"unreachable": "unnameable", "env": null})),
            key("b", json!({"text_constraint": {"type": "string"}}))
        ])),
        &no_external(),
    );
    let once = render::render(&contract, Format::Contract, &Options::default()).expect("renders");
    let twice = render::render(
        &Contract::from_json(&once).expect("reads"),
        Format::Contract,
        &Options::default(),
    )
    .expect("renders");
    assert_eq!(once, twice);
}

// ---------------------------------------------------------------------------------------------
// The meta-schema this binary embeds
// ---------------------------------------------------------------------------------------------

#[test]
fn the_embedded_meta_schema_refuses_what_it_should() {
    let missing_producer = r#"{"terrace_contract": 1, "app": {"name": "x"}}"#;
    let errors = validate::validate(missing_producer).expect("the meta-schema compiles");
    assert!(
        !errors.is_empty(),
        "a document with no `producer` is refused"
    );
}

#[test]
fn validate_reports_where_rather_than_only_what() {
    let errors = validate::validate(r#"{"terrace_contract": "one"}"#).expect("compiles");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("terrace_contract")),
        "{errors:?}"
    );
}

#[test]
fn a_violation_names_a_location_a_reader_can_follow() {
    let contract = document(
        &schema(&json!([key(
            "token",
            json!({"secret": true, "default_value": PLACEHOLDER_DEFAULT})
        )])),
        &no_external(),
    );
    let violations = conform::conform(&contract, Tier::Document);
    let refusal = violations
        .iter()
        .find(|violation| violation.rule == "refusal 6")
        .expect("the refusal fires");
    assert_eq!(refusal.at, "/schema/keys/0");
    // A violation's detail names the *key*, never the value — which is what makes it printable at
    // all. `refusal 6` exists because a document is meant to be safe to read, and a report that
    // quoted the credential to say so would undo the rule it was enforcing.
    assert!(refusal.detail.contains("token"), "{refusal}");
    assert!(!refusal.detail.contains(PLACEHOLDER_DEFAULT), "{refusal}");
}

#[test]
fn the_unreachable_reasons_stay_distinguishable() {
    // The distinction a consumer acts on: `unnameable` means skip the key, `indirection` means the
    // producer should have refused to build. Reading either as the other is a deployment.
    let contract = document(
        &schema(&json!([
            key("a", json!({"env": null, "unreachable": "unnameable"})),
            key("b", json!({"env": null, "unreachable": "indirection"}))
        ])),
        &no_external(),
    );
    assert_eq!(
        contract.schema.keys[0].unreachable,
        Some(Unreachable::Unnameable)
    );
    assert_eq!(
        contract.schema.keys[1].unreachable,
        Some(Unreachable::Indirection)
    );
}
