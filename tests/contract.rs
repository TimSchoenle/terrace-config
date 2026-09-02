//! The contract export: the document a build publishes, and the claims it refuses to publish.
//!
//! These tests exist for a different reason from `schema.rs`'s. There, a wrong answer produces
//! documentation that reads wrong. Here it produces a document a deployment pipeline *acts* on —
//! and every way this file can be wrong is a way a gate stops catching what it was built to catch.
//! So the assertions are about the two properties a consumer has no way to check for itself: that
//! the document is byte-stable, and that the declared external surface cannot be used to exempt a
//! real configuration key from validation.

#![cfg(feature = "schema")]
#![expect(dead_code, reason = "fixtures are read by the derive, not at runtime")]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use serde_json::{Value as Json, json};
use terrace_config::Terrace;
use terrace_config::schema::{
    App, CONTRACT_VERSION, Contract, DEFAULT_PATH, DRAFT_07, Describe, External, ExternalVar,
    JsonSchema, LABEL_PATH, LABEL_PREFIX, LABEL_VERSION, LoaderRole, Schema, TextForm, Unknown,
};

#[derive(Deserialize, Serialize, Default, Describe)]
struct Config {
    /// Bundle directory the readiness probe checks.
    #[serde(default = "default_dist")]
    dist_dir: String,
    /// Revalidation interval in seconds.
    #[serde(default)]
    ttl_secs: u64,
    #[config(nested)]
    github: Github,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Github {
    /// User whose repositories are listed.
    ///
    /// `userName` is deliberately unspellable in the environment — a key folded to lower case on
    /// the way in never comes back — so this field carries one alias that has environment and file
    /// spellings and one that has neither.
    #[serde(alias = "user", alias = "userName")]
    username: String,
    /// Bearer token lifting the rate limit.
    #[config(secret)]
    #[serde(skip_serializing)]
    token: Option<String>,
}

fn default_dist() -> String {
    "public".to_owned()
}

/// The loader every test here describes against — a reserved variable included, because a reserved
/// variable is one of the two ways an external declaration can collide with the loader.
fn terrace() -> Terrace {
    Terrace::new("PORTFOLIO_").reserve("PORTFOLIO_PROFILE")
}

fn schema() -> Schema {
    terrace()
        .schema::<Config>()
        .with_defaults_from(&Config::default())
        .expect("the default config serialises")
}

fn contract() -> Contract {
    schema()
        .into_contract(App::new("portfolio").version("v2.5.0"))
        .build()
        .expect("the contract has nothing to refuse")
}

// ---------------------------------------------------------------------------------------------
// The envelope
// ---------------------------------------------------------------------------------------------

#[test]
fn the_envelope_carries_both_machine_readable_halves() {
    let json: Json = serde_json::from_str(&contract().to_json().expect("renders")).expect("parses");

    assert_eq!(json["terrace_contract"], Json::from(CONTRACT_VERSION));
    assert_eq!(json["app"]["name"], Json::from("portfolio"));
    assert_eq!(json["app"]["version"], Json::from("v2.5.0"));
    // The schema half is what lets a validator check an environment variable and a secrets file
    // name; the JSON Schema half is the only one a stock validator can act on. Publishing one
    // without the other is what the envelope exists to prevent.
    assert!(json["schema"]["keys"].is_array());
    assert!(json["json_schema"]["properties"].is_object());
    assert_eq!(json["external"]["unknown"], Json::from("reject"));
}

#[test]
fn a_field_nothing_was_said_about_is_absent_rather_than_null() {
    let json: Json = serde_json::from_str(&contract().to_json().expect("renders")).expect("parses");

    assert!(json["app"].get("revision").is_none());
}

#[test]
fn nothing_in_the_document_names_the_image() {
    let json: Json = serde_json::from_str(&contract().to_json().expect("renders")).expect("parses");

    // Deliberate. A digest is what building the image produces, so a field carrying it could only
    // be written after the push, changing bytes that were already hashed and already committed.
    // The tie is the attachment: whatever comes back from asking a digest for its `ARTIFACT_TYPE`
    // referrers is that digest's contract, content-addressed by the registry.
    // Scoped to `app`, not to the whole document: a `///` comment on a key about image pinning
    // or checksum verification would legitimately contain `sha256:`, and an assertion that fails
    // on a fixture's prose is testing the fixture.
    let app = serde_json::to_string(&json["app"]).expect("renders");
    assert!(!app.contains("digest"), "{app}");
    assert!(!app.contains("sha256"), "{app}");
}

#[test]
fn the_document_is_byte_stable() {
    // The whole publication scheme rests on this: the document is hashed into an image label, and
    // three copies of it are cross-checked against that hash. A generator that produced two
    // different byte strings for one source tree would make every one of those checks a coin toss.
    let first = contract().to_json().expect("renders");
    let second = contract().to_json().expect("renders");
    assert_eq!(first, second);
}

#[test]
fn it_round_trips_through_serde() {
    let rendered = contract().to_json().expect("renders");
    let parsed: Contract = serde_json::from_str(&rendered).expect("a consumer can read it back");

    assert_eq!(parsed.to_json().expect("renders"), rendered);
    assert_eq!(parsed, contract());
}

#[test]
fn the_json_schema_half_is_the_json_schema_rendering() {
    // Not a tautology: the two are produced by one function precisely so that they cannot drift,
    // and this is the test that fails if someone re-implements one of them.
    let options = JsonSchema::new()
        .meta_schema(DRAFT_07)
        .closed(true)
        // The contract's defaults, restated: a contract asserts nothing about which layer supplies
        // a required key, so its `required` lists — and the `anyOf` an aliased required key would
        // produce — are both off.
        .require_present(false)
        .title("portfolio configuration");
    let standalone: Json =
        serde_json::from_str(&schema().to_json_schema_with(&options).expect("renders"))
            .expect("parses");

    assert_eq!(contract().json_schema, standalone);
}

#[test]
fn it_defaults_to_the_dialect_helm_reads_and_the_strictness_that_catches_a_rename() {
    let contract = contract();

    assert_eq!(contract.json_schema["$schema"], Json::from(DRAFT_07));
    // Closed: an unknown key in a rendered configuration is the defect this document exists to
    // catch, and an open schema catches none of them.
    assert_eq!(
        contract.json_schema["additionalProperties"],
        Json::from(false)
    );
}

#[test]
fn the_title_falls_back_to_the_app_but_never_overrides_one() {
    assert_eq!(
        contract().json_schema["title"],
        Json::from("portfolio configuration")
    );

    let titled = schema()
        .into_contract(App::new("portfolio"))
        .title("chosen")
        .build()
        .expect("builds");
    assert_eq!(titled.json_schema["title"], Json::from("chosen"));
}

#[test]
fn overriding_a_knob_cannot_change_the_dialect() {
    // The builder used to take a whole `JsonSchema`, so the documented way to relax one knob —
    // `.json_schema(JsonSchema::new().closed(false))` — silently took the dialect back to 2020-12
    // with it. That validates fine alone and fails only where it is expensive: against a pipeline
    // pinning a draft-07 engine, or when two contracts of one document refuse to merge.
    for contract in [
        schema()
            .into_contract(App::new("portfolio"))
            .closed(false)
            .build()
            .expect("builds"),
        schema()
            .into_contract(App::new("portfolio"))
            .title("whatever")
            .build()
            .expect("builds"),
    ] {
        assert_eq!(contract.json_schema["$schema"], Json::from(DRAFT_07));
    }
}

#[test]
fn closed_is_the_knob_it_says_it_is() {
    let open = schema()
        .into_contract(App::new("portfolio"))
        .closed(false)
        .build()
        .expect("builds");

    assert!(open.json_schema.get("additionalProperties").is_none());
}

#[test]
fn a_reserved_key_is_in_the_schema_half_and_out_of_the_json_schema() {
    let contract = contract();

    // The two halves answer different questions and the reserved variable is where they diverge:
    // the loader does read `PORTFOLIO_PROFILE`, so a validator checking a pod's environment must
    // know about it — and no file may supply it, so a schema completing it in a config file would
    // offer a key that does nothing.
    assert!(
        contract
            .schema
            .loader
            .iter()
            .any(|var| var.env == "PORTFOLIO_PROFILE")
    );
    assert!(contract.json_schema["properties"].get("profile").is_none());
}

#[test]
fn the_labels_are_constants_a_dockerfile_can_carry_verbatim() {
    // The whole reason the hash label is gone: with it, one of the four had to be interpolated
    // from a build argument fed by a host-side run of the generator, which a multi-stage build
    // cannot do without running the generator twice. These three never change for a service.
    let rendered = contract().to_dockerfile_labels(DEFAULT_PATH);

    assert_eq!(
        rendered,
        concat!(
            "LABEL dev.terrace.config.contract.version=\"1\" \\\n",
            "      dev.terrace.config.contract.path=\"/config/contract.json\" \\\n",
            "      dev.terrace.config.prefix=\"PORTFOLIO_\"\n",
        )
    );
    // A following instruction needs no separator of its own.
    assert!(rendered.ends_with('\n') && !rendered.ends_with("\\\n"));
}

#[test]
fn a_built_image_is_checked_against_the_contract_rather_than_the_dockerfile() {
    let contract = contract();
    let mut labels: BTreeMap<String, String> = contract
        .labels(DEFAULT_PATH)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    // An image carries `org.opencontainers.image.*` and whatever its base contributed. None of
    // that is this document's business.
    labels.insert(
        "org.opencontainers.image.title".to_owned(),
        "portfolio".to_owned(),
    );

    contract
        .verify_labels(DEFAULT_PATH, &labels)
        .expect("the labels this contract produced are the ones it accepts");

    // A build argument that failed to interpolate — the failure a source diff cannot see, because
    // the Dockerfile is correct and the image is not.
    let mut wrong = labels.clone();
    wrong.insert(LABEL_PREFIX.to_owned(), String::new());
    let error = contract
        .verify_labels(DEFAULT_PATH, &wrong)
        .expect_err("refused");
    assert!(error.to_string().contains(LABEL_PREFIX), "{error}");

    let mut missing = labels.clone();
    missing.remove(LABEL_PATH);
    let error = contract
        .verify_labels(DEFAULT_PATH, &missing)
        .expect_err("refused");
    assert!(
        error
            .to_string()
            .contains("no `dev.terrace.config.contract.path`"),
        "{error}"
    );
}

#[test]
fn the_labels_name_themselves_so_a_dockerfile_never_has_to() {
    let labels = contract().labels(DEFAULT_PATH);

    assert_eq!(
        labels,
        vec![
            (LABEL_VERSION, CONTRACT_VERSION.to_string()),
            (LABEL_PATH, DEFAULT_PATH.to_owned()),
            (LABEL_PREFIX, "PORTFOLIO_".to_owned()),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// The external surface
// ---------------------------------------------------------------------------------------------

fn with_external(external: External) -> Result<Contract, terrace_config::Error> {
    schema()
        .into_contract(App::new("portfolio"))
        .external(external)
        .build()
}

#[test]
fn a_declared_variable_is_described_well_enough_to_be_checked() {
    let contract = with_external(
        External::new().var(
            ExternalVar::new("PORT")
                .owner("dioxus")
                .ty("u16")
                .default("8080")
                .docs("Bind port."),
        ),
    )
    .expect("builds");

    let port = &contract.external.env[0];
    assert_eq!(port.name, "PORT");
    assert_eq!(port.ty.as_deref(), Some("u16"));
    assert_eq!(port.default.as_deref(), Some("8080"));
    assert_eq!(port.owner.as_deref(), Some("dioxus"));
    // The point of carrying the type: a chart passing `PORT: "http"` fails the same gate a chart
    // passing a bad configuration value fails. A suppression list could not say this.
    assert!(!port.required);
}

#[test]
fn a_variable_inside_the_loaders_prefix_is_refused() {
    // The one that matters. Everything in the prefix is a configuration key, so declaring one
    // external would leave it governed and exempt at once — and the exemption would win, silently
    // removing exactly the key a rename is most likely to break.
    let error = with_external(External::new().var(ExternalVar::new("PORTFOLIO_ISR__TTL_SECS")))
        .expect_err("refused");

    let message = error.to_string();
    assert!(message.contains("PORTFOLIO_ISR__TTL_SECS"), "{message}");
    assert!(message.contains("PORTFOLIO_"), "{message}");
}

#[test]
fn a_variable_colliding_with_a_reserved_one_is_refused() {
    // The other door to the same defect: a reserved variable is read by the loader and carries the
    // prefix, but a renamed prefix or a `reserve` of an unprefixed name puts it outside the check
    // above.
    let error = with_external(External::new().var(ExternalVar::new("PORTFOLIO_PROFILE")))
        .expect_err("refused");
    assert!(error.to_string().contains("PORTFOLIO_PROFILE"));
}

#[test]
fn a_variable_declared_twice_is_refused() {
    let error = with_external(
        External::new()
            .var(ExternalVar::new("PORT").ty("u16"))
            .var(ExternalVar::new("PORT").ty("String")),
    )
    .expect_err("refused");

    // Two descriptions of one variable, and a consumer would have to pick. Refusing to build beats
    // picking, on `Schema::merge`'s reasoning.
    assert!(error.to_string().contains("twice"));
}

#[test]
fn a_name_the_environment_cannot_hold_is_refused() {
    for name in ["1PORT", "PORT-NUMBER", "", "PORT NUMBER"] {
        let error = with_external(External::new().var(ExternalVar::new(name)))
            .expect_err(&format!("`{name}` was accepted"));
        assert!(error.to_string().contains("environment"), "{name}");
    }
}

#[test]
fn an_unprefixed_variable_is_accepted_even_when_it_looks_like_a_key() {
    // `CONFIG` is not `PORTFOLIO_CONFIG`. The check is on the spelling the loader actually reads,
    // never on a resemblance to one.
    with_external(External::new().var(ExternalVar::new("CONFIG"))).expect("builds");
}

#[test]
fn an_ignore_pattern_may_not_reach_into_the_loaders_namespace() {
    // The hole `External::var`'s refusal closes, reached through the other door. `var` at least
    // names the variable it exempts; a pattern exempts everything it happens to cover.
    for pattern in [
        "PORTFOLIO_*",
        "PORTFOLIO_ISR__*",
        "PORTFOLIO_GITHUB__TOKEN",
        // The nastiest spelling, because it carries no prefix and reads as a pattern about the
        // external `PORT`: one character from correct, and it disables the whole gate.
        "PORT*",
        "P*",
    ] {
        let error = with_external(External::new().ignore(pattern))
            .expect_err(&format!("`{pattern}` was accepted"));
        assert!(error.to_string().contains("PORTFOLIO_"), "{pattern}");
    }
}

/// A loader whose variables are deliberately spelled outside the prefix, which is what
/// `config_var`, `secrets_dir_var` and `reserve` are for and what this crate's own README shows.
fn renamed() -> Terrace {
    Terrace::new("PORTFOLIO_")
        .config_var("APP_CONFIG_PATH")
        .secrets_dir_var("CREDENTIALS_DIR")
        .reserve("DEPLOY_PROFILE")
}

#[test]
fn an_ignore_pattern_may_not_cover_a_variable_the_loader_reads() {
    // The prefix is not the whole namespace. A *key's* environment spelling is derived from the
    // prefix and so is always caught by the check above; a *loader* variable's is whatever the
    // caller passed. Exempting one of those is worse than exempting a key — `CREDENTIALS_DIR`
    // decides where every credential is read from, so a chart misspelling it loses all of them at
    // once, silently.
    for pattern in [
        "CREDENTIALS_*",
        "CREDENTIALS_DIR",
        "APP_CONFIG_PATH",
        "DEPLOY_*",
        "APP_*",
    ] {
        let error = renamed()
            .schema::<Config>()
            .with_defaults_from(&Config::default())
            .expect("serialises")
            .into_contract(App::new("portfolio"))
            .external(External::new().ignore(pattern))
            .build()
            .expect_err(&format!("`{pattern}` was accepted"));
        assert!(error.to_string().contains("the loader reads"), "{pattern}");
    }
}

#[test]
fn a_pattern_beside_the_loaders_variables_is_still_fine() {
    // The mirror-image mistake would be a rule so broad it rejects correct declarations. None of
    // these covers a spelling the loader reads.
    renamed()
        .schema::<Config>()
        .with_defaults_from(&Config::default())
        .expect("serialises")
        .into_contract(App::new("portfolio"))
        .external(
            External::new()
                .ignore("CREDENTIAL")
                .ignore("APP_CONFIG_PATHS")
                .ignore("DEPLOYMENT_*"),
        )
        .build()
        .expect("builds");
}

#[test]
fn a_variable_cannot_be_declared_and_ignored_at_once() {
    // The duplicate-declaration case in different words: one says a chart's value for it is
    // checked, the other says it is nobody's business, and only the classification order decides
    // which. `build` refuses less than that elsewhere.
    let error = with_external(
        External::new()
            .var(ExternalVar::new("PORT").ty("u16"))
            .ignore("PORT"),
    )
    .expect_err("refused");
    assert!(error.to_string().contains("PORT"));
}

#[test]
fn a_wildcard_that_happens_to_cover_a_declared_variable_is_left_alone() {
    // `ignore("KUBERNETES_*")` beside a declared `KUBERNETES_SERVICE_HOST` is an ordinary thing to
    // write, and the ordered list already resolves it — step 5 beats step 6. Refusing it would
    // make that ordering carry no weight.
    with_external(
        External::new()
            .var(ExternalVar::new("KUBERNETES_SERVICE_HOST").ty("String"))
            .ignore("KUBERNETES_*"),
    )
    .expect("builds");
}

#[test]
fn an_exact_pattern_the_prefix_merely_starts_with_is_fine() {
    // `PORT` matches the name `PORT` and nothing else, and no key is spelled that. Refusing it
    // would be the mirror-image mistake: a rule so broad it rejects correct declarations.
    with_external(External::new().ignore("PORT")).expect("builds");
    with_external(External::new().ignore("KUBERNETES_*").ignore("HOSTNAME")).expect("builds");
}

#[test]
fn an_ignore_pattern_is_a_trailing_star_or_nothing() {
    with_external(External::new().ignore("KUBERNETES_").ignore("KUBERNETES_*"))
        .expect("both forms build");

    for pattern in ["", "*", "A*B", "A-*"] {
        let error = with_external(External::new().ignore(pattern))
            .expect_err(&format!("`{pattern}` was accepted"));
        // Every consumer of this document implements the matching itself, in whatever language it
        // is written in. A pattern language is a place for two of them to disagree about what is
        // exempt from a check.
        assert!(!error.to_string().is_empty(), "{pattern}");
    }
}

#[test]
fn a_bare_star_points_at_the_option_that_says_what_it_means() {
    let error = with_external(External::new().ignore("*")).expect_err("refused");
    assert!(error.to_string().contains("Unknown::Allow"));
}

#[test]
fn the_unknown_policy_defaults_to_refusing() {
    assert_eq!(contract().external.unknown, Unknown::Reject);
    assert_eq!(
        with_external(External::new().unknown(Unknown::Allow))
            .expect("builds")
            .external
            .unknown,
        Unknown::Allow
    );
}

// ---------------------------------------------------------------------------------------------
// What it refuses to publish
// ---------------------------------------------------------------------------------------------

#[test]
fn a_secret_key_carries_no_default_out_of_the_box() {
    let contract = contract();
    let token = contract
        .schema
        .keys
        .iter()
        .find(|key| key.path == "github.token")
        .expect("described");

    assert!(token.secret);
    assert!(token.default_value.is_none());
    // And the JSON Schema half agrees, rather than each half redacting on its own account.
    assert!(
        contract.json_schema["properties"]["github"]["properties"]["token"]
            .get("default")
            .is_none()
    );
}

#[test]
fn a_hand_built_secret_default_is_refused_at_the_boundary() {
    // Nothing in this crate produces this pair — `with_defaults_from` drops a secret's value. The
    // check is here anyway because this is the point the document crosses into a public registry,
    // and "no code path produces it" is a weaker guarantee than "the type will not carry it".
    let mut schema = schema();
    for key in &mut schema.keys {
        if key.secret {
            key.default = Some("ghp_realcredential".to_owned());
        }
    }

    let error = schema
        .into_contract(App::new("portfolio"))
        .build()
        .expect_err("refused");
    assert!(error.to_string().contains("github.token"));
    // The message names the key, never the value.
    assert!(!error.to_string().contains("ghp_realcredential"));
}

#[test]
fn a_secret_with_a_default_is_refused_in_either_order() {
    // One intent, one outcome. Dropping the default inside `secret()` would make the first of
    // these build and the second fail, and the caller writing either has a misunderstanding worth
    // surfacing rather than silently repairing.
    for var in [
        ExternalVar::new("API_TOKEN").default("hunter2").secret(),
        ExternalVar::new("API_TOKEN").secret().default("hunter2"),
    ] {
        let error = with_external(External::new().var(var)).expect_err("refused");
        assert!(error.to_string().contains("API_TOKEN"));
        assert!(!error.to_string().contains("hunter2"));
    }
}

// ---------------------------------------------------------------------------------------------
// The constraints, which are what makes the document checkable by something that is not Rust
// ---------------------------------------------------------------------------------------------

#[test]
fn every_key_carries_what_its_value_must_be() {
    let contract = contract();
    let key = |path: &str| {
        contract
            .schema
            .keys
            .iter()
            .find(|key| key.path == path)
            .unwrap_or_else(|| panic!("{path} is described"))
            .constraint
            .clone()
    };

    // Without this a consumer has only `ty`, a Rust type name with no published vocabulary, and
    // every consumer in every language writes the same mapping table by reading the app's source.
    assert_eq!(
        key("ttl_secs"),
        Some(json!({ "type": "integer", "minimum": 0 }))
    );
    assert_eq!(key("dist_dir"), Some(json!({ "type": "string" })));
    assert_eq!(key("github.username"), Some(json!({ "type": "string" })));
}

#[test]
fn a_declared_variable_carries_one_too() {
    // The half that has no JSON Schema to fall back on. Every value in an environment is a string,
    // so this is the only thing that makes `PORT: "http"` catchable.
    let contract =
        with_external(External::new().var(ExternalVar::new("PORT").ty("u16"))).expect("builds");

    assert_eq!(
        contract.external.env[0].constraint,
        Some(json!({ "type": "integer", "minimum": 0, "maximum": 65535 }))
    );
}

#[test]
fn a_type_nothing_can_interpret_leaves_the_constraint_absent_rather_than_guessed() {
    let contract = with_external(External::new().var(ExternalVar::new("ENDPOINT").ty("MyNewtype")))
        .expect("builds");

    // Absent means "declared but unconstrained", which is nearer to `ignore` than the declaration
    // reads — and inventing a constraint for a type this crate does not recognise would reject
    // values the image accepts, which is the one thing a schema here must never do.
    assert_eq!(contract.external.env[0].constraint, None);
    assert_eq!(contract.external.env[0].ty.as_deref(), Some("MyNewtype"));
}

#[test]
fn a_choice_constrains_to_its_spellings_rather_than_to_a_type() {
    let contract =
        with_external(External::new().var(ExternalVar::new("MODE").values(["fast", "slow"])))
            .expect("builds");

    assert_eq!(
        contract.external.env[0].constraint,
        Some(json!({ "type": "string", "enum": ["fast", "slow"] }))
    );
}

#[test]
fn the_flat_constraint_agrees_with_the_nested_one() {
    // Two renderings of one fact, so the test that matters is that they cannot disagree: both come
    // from `json_schema::constraint`, and this is what fails if one of them stops.
    let contract = contract();
    let nested = &contract.json_schema["properties"]["ttl_secs"];
    let flat = contract
        .schema
        .keys
        .iter()
        .find(|key| key.path == "ttl_secs")
        .and_then(|key| key.constraint.clone())
        .expect("carried");

    for (keyword, value) in flat.as_object().expect("an object") {
        assert_eq!(&nested[keyword], value, "{keyword}");
    }
}

// ---------------------------------------------------------------------------------------------
// The text constraints, which are what an environment variable is actually checked against
// ---------------------------------------------------------------------------------------------

fn key_of<'a>(contract: &'a Contract, path: &str) -> &'a terrace_config::schema::Key {
    contract
        .schema
        .keys
        .iter()
        .find(|key| key.path == path)
        .unwrap_or_else(|| panic!("{path} is described"))
}

#[test]
fn an_integer_key_says_what_its_text_must_look_like() {
    let contract = contract();

    // The document-space constraint describes the parsed value; `"0"` fails it under every
    // conforming validator, which is why a second one is needed at all.
    assert_eq!(
        key_of(&contract, "ttl_secs").constraint,
        Some(json!({ "type": "integer", "minimum": 0 }))
    );
    assert_eq!(
        key_of(&contract, "ttl_secs").text_constraint,
        Some(json!({ "type": "string", "pattern": r"^\s*\+?[0-9]+\s*$" }))
    );
}

#[test]
fn the_text_pattern_admits_everything_the_loader_admits() {
    let pattern = key_of(&contract(), "ttl_secs")
        .text_constraint
        .clone()
        .expect("carried");
    let pattern = pattern["pattern"].as_str().expect("a string");

    // Measured against the loader rather than reasoned from TOML's grammar: figment's `Env`
    // provider takes all of these for a `u64` key, so a pattern rejecting one would stop a
    // deployment that was correct — the failure this whole module is written to avoid.
    for text in ["0", "42", "007", "+5", " 7", "7 "] {
        assert!(matches_pattern(pattern, text), "{text} must be admitted");
    }
    // And refuses all of these, so rejecting them costs nothing and catches a real mistake.
    for text in ["-1", "1_000", "0x1F", "1e3", "", "http"] {
        assert!(!matches_pattern(pattern, text), "{text} must be refused");
    }
}

#[test]
fn a_string_key_carries_no_text_constraint() {
    // An environment value is a string already, so `{"type": "string"}` would constrain nothing.
    // `None` says the same thing without inviting a consumer to think a check happened.
    assert_eq!(key_of(&contract(), "dist_dir").text_constraint, None);
    assert_eq!(key_of(&contract(), "github.username").text_constraint, None);
}

#[test]
fn a_declared_variable_is_checkable_against_the_text_it_actually_carries() {
    // `PORT: "http"` is the motivating case, and it is catchable because of this field rather
    // than because of `constraint`: an external variable is only ever text.
    let contract =
        with_external(External::new().var(ExternalVar::new("PORT").ty("u16"))).expect("builds");
    let var = &contract.external.env[0];

    assert_eq!(
        var.constraint,
        Some(json!({ "type": "integer", "minimum": 0, "maximum": 65535 }))
    );
    let pattern = var.text_constraint.as_ref().expect("carried")["pattern"]
        .as_str()
        .expect("a string");
    assert!(matches_pattern(pattern, "8080"));
    assert!(!matches_pattern(pattern, "http"));
}

#[derive(Deserialize, Serialize, Default, Describe)]
#[serde(rename_all = "lowercase")]
enum Level {
    Off,
    #[default]
    Info,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Choices {
    /// How much.
    #[config(values)]
    #[serde(default)]
    level: Level,
    /// A flag.
    #[serde(default)]
    flag: bool,
}

#[test]
fn a_choice_admits_the_whitespace_the_environment_layer_trims() {
    // The two spaces differ, and a bare enum in both fields was wrong in one of them. Measured:
    // figment's Env provider trims, so an environment value of `info` with a trailing space, a
    // leading space, or surrounding tabs and newlines all load. The document layer trims nothing,
    // so the same spelling inside a TOML string really is refused — which is why `constraint`
    // keeps the bare set and only the text form widens.
    let schema = Terrace::new("CHOICES_").schema::<Choices>();
    let key = |path: &str| {
        schema
            .keys
            .iter()
            .find(|key| key.path == path)
            .unwrap_or_else(|| panic!("no such key"))
    };

    assert_eq!(
        key("level").constraint,
        Some(json!({ "type": "string", "enum": ["off", "info"] }))
    );
    assert_eq!(
        key("level").text_constraint,
        Some(json!({ "type": "string", "pattern": PATTERN_LEVEL }))
    );

    // A boolean is the same shape and was measured the same way: `true` and `false` load with
    // surrounding whitespace, and `TRUE` does not.
    assert_eq!(key("flag").constraint, Some(json!({ "type": "boolean" })));
    assert_eq!(
        key("flag").text_constraint,
        Some(json!({ "type": "string", "pattern": PATTERN_FLAG }))
    );
}

const PATTERN_LEVEL: &str = r"^\s*(off|info)\s*$";
const PATTERN_FLAG: &str = r"^\s*(true|false)\s*$";
const PATTERN_AWKWARD: &str = r"^\s*(a\.b|c\+d)\s*$";

#[derive(Deserialize, Serialize, Default, Describe)]
enum Awkward {
    #[default]
    #[serde(rename = "a.b")]
    Dotted,
    #[serde(rename = "c+d")]
    Plus,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Renamed {
    /// A choice whose spellings carry regular-expression metacharacters.
    #[config(values)]
    #[serde(default)]
    mode: Awkward,
}

#[test]
fn a_spelling_carrying_regex_metacharacters_is_escaped() {
    // A rename takes any string, so a variant can carry a dot or a plus. Left unescaped, the first
    // would match any character in that position — accepting a spelling the loader refuses — and
    // the second would make the preceding character optional. Escaping the ECMA-262 metacharacter
    // set is always correct, and getting it wrong in the strict direction would reject a
    // deployment that works.
    let schema = Terrace::new("RENAMED_").schema::<Renamed>();
    let pattern = schema.keys[0].text_constraint.as_ref().expect("carried")["pattern"]
        .as_str()
        .expect("a string value");

    assert_eq!(pattern, PATTERN_AWKWARD);
}

#[test]
fn a_choice_states_its_spellings_in_both_spaces_differently() {
    // Not redundant with `constraint`, and not a copy of it either: a consumer reading an absent
    // text constraint as "unconstrained" would lose the check entirely on the one layer where
    // every value is text, and a copy of the bare enum would refuse the whitespace that layer
    // trims before it compares.
    let contract =
        with_external(External::new().var(ExternalVar::new("MODE").values(["fast", "slow"])))
            .expect("builds");
    let var = &contract.external.env[0];

    assert_eq!(
        var.constraint,
        Some(json!({ "type": "string", "enum": ["fast", "slow"] }))
    );
    assert_eq!(
        var.text_constraint,
        Some(json!({ "type": "string", "pattern": PATTERN_MODE }))
    );
}

const PATTERN_MODE: &str = r"^\s*(fast|slow)\s*$";

#[test]
fn a_stated_constraint_survives_the_derive() {
    // The escape hatch for a domain type. Deriving over the top of it would leave a `Duration` or
    // a connection string permanently uncheckable, which is the gap `constraint: None` documents
    // and this is the way out of.
    let contract = with_external(External::new().var(
        ExternalVar::new("TIMEOUT").ty("Duration").constraint(
            Some(json!({ "type": "string" })),
            Some(json!({ "type": "string", "pattern": "^[0-9]+(ms|s|m)$" })),
        ),
    ))
    .expect("builds");

    let var = &contract.external.env[0];
    assert_eq!(var.constraint, Some(json!({ "type": "string" })));
    assert_eq!(
        var.text_constraint,
        Some(json!({ "type": "string", "pattern": "^[0-9]+(ms|s|m)$" }))
    );
}

/// A deliberately tiny matcher for the anchored patterns this crate emits.
///
/// Not a regex engine: every pattern here is `^\s*<sign><digits>\s*$`, and pulling in a regex
/// crate as a dev-dependency to assert six strings against one shape would be a dependency in the
/// manifest for a property this can state directly.
fn matches_pattern(pattern: &str, text: &str) -> bool {
    assert!(
        pattern.starts_with(r"^\s*") && pattern.ends_with(r"\s*$"),
        "this matcher only understands the shape this crate emits: {pattern}"
    );
    let signed = pattern.contains("[-+]?");
    let text = text.trim();
    let digits = match text.strip_prefix('+') {
        Some(rest) => rest,
        None => match text.strip_prefix('-') {
            Some(rest) if signed => rest,
            Some(_) => return false,
            None => text,
        },
    };
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

// ---------------------------------------------------------------------------------------------
// Requiredness, which the document cannot establish and does not claim to
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize, Serialize, Default, Describe)]
struct Mandatory {
    /// A mandatory credential — the ordinary shape of one.
    #[config(secret)]
    endpoint: String,
}

#[test]
fn a_contract_marks_nothing_required_in_its_json_schema() {
    // Measured: the loader takes a required key from the document, from the environment, or from a
    // secrets file, and refuses only when nothing supplies it. JSON Schema's `required` says the
    // *document* must carry it, so a chart supplying a required secret from a mount — the only way
    // to supply a secret — renders a document a `required` list refuses and a deployment that
    // starts. A false rejection of the arrangement the secrets-directory layer exists for.
    let contract = Terrace::new("MANDATORY_")
        .schema::<Mandatory>()
        .into_contract(App::new("mandatory"))
        .build()
        .expect("builds");

    assert!(contract.json_schema.get("required").is_none());
    // The fact a consumer checks instead, published per key, checkable across every layer.
    assert!(contract.schema.keys[0].required);
}

#[test]
fn the_standalone_rendering_still_marks_them() {
    // Its reader is an editor validating a hand-written `config.toml`, where the document is the
    // only layer there is. Nothing about this change touches that.
    let rendered = Terrace::new("MANDATORY_")
        .schema::<Mandatory>()
        .to_json_schema()
        .expect("renders");
    let json: Json = serde_json::from_str(&rendered).expect("parses");

    assert_eq!(json["required"], json!(["endpoint"]));
}

#[test]
fn requiredness_can_be_switched_back_on() {
    let contract = Terrace::new("MANDATORY_")
        .schema::<Mandatory>()
        .into_contract(App::new("mandatory"))
        .require_present(true)
        .build()
        .expect("builds");

    assert_eq!(contract.json_schema["required"], json!(["endpoint"]));
}

// ---------------------------------------------------------------------------------------------
// Aliases, which are live spellings in every layer and not only in the document
// ---------------------------------------------------------------------------------------------

#[test]
fn an_alias_is_published_in_every_spelling_that_supplies_it() {
    // Measured: with `#[serde(alias = "user")]` on `github.username`, the loader answers to
    // `PORTFOLIO_GITHUB__USER` in the environment and to a secrets file named `github__user`,
    // exactly as it answers to the canonical spellings.
    //
    // An alias is what a maintainer adds when renaming a key so existing deployments keep working.
    // Publishing only the canonical spelling would send the old name to the ordered list's step 4
    // — "a key spelling nothing in this image reads" — and reject a correct deployment, turning
    // the shim that makes a rename safe into the thing that fails the gate.
    let contract = contract();
    let key = key_of(&contract, "github.username");

    assert_eq!(key.aliases, ["github.user", "github.userName"]);
    assert_eq!(key.env_aliases, ["PORTFOLIO_GITHUB__USER"]);
    assert_eq!(key.env_file_aliases, ["PORTFOLIO_GITHUB__USER_FILE"]);
    assert_eq!(key.secrets_file_aliases, ["github__user"]);
}

#[test]
fn a_key_without_aliases_carries_no_alias_spellings() {
    let contract = contract();
    let key = key_of(&contract, "ttl_secs");

    assert!(key.aliases.is_empty());
    assert!(key.env_aliases.is_empty());
    assert!(key.env_file_aliases.is_empty());
    assert!(key.secrets_file_aliases.is_empty());
}

#[test]
fn the_alias_sets_are_on_every_key_whether_or_not_it_has_any() {
    // They are the fields the ordered list consults for every variable on every container, so a
    // consumer reaching them has not yet decided which key it is holding — the worst place to hand
    // it two shapes. And unlike `constraint` and `default`, which are omitted when unset because
    // absence *means* something there, an absent list here would say exactly what an empty one
    // says. `aliases`, which these are derived from, is always present too.
    let rendered = contract().to_json().expect("renders");
    let json: Json = serde_json::from_str(&rendered).expect("parses");
    let keys = json["schema"]["keys"].as_array().expect("an array");

    let fields = |key: &Json| {
        let mut names: Vec<String> = key
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        names.sort();
        names
    };

    let with = keys
        .iter()
        .find(|key| !key["aliases"].as_array().expect("an array").is_empty())
        .expect("a key with aliases");
    let without = keys
        .iter()
        .find(|key| key["aliases"].as_array().expect("an array").is_empty())
        .expect("a key without");

    assert_eq!(fields(with), fields(without));
    for name in ["env_aliases", "env_file_aliases", "secrets_file_aliases"] {
        assert!(
            without[name].is_array(),
            "{name} is absent on a key with none"
        );
    }
}

#[test]
fn an_alias_with_no_environment_spelling_contributes_nothing() {
    // Which is why the three lists are membership sets rather than arrays parallel to `aliases`:
    // `userName` is folded to lower case on the way in and never comes back, so it has no
    // environment spelling at all. A consumer indexing `env_aliases` by an `aliases` position
    // would read the wrong one.
    let contract = contract();
    let key = key_of(&contract, "github.username");

    assert_eq!(key.aliases, ["github.user", "github.userName"]);
    assert_eq!(key.env_aliases, ["PORTFOLIO_GITHUB__USER"]);
    assert_eq!(key.secrets_file_aliases, ["github__user"]);
}

// ---------------------------------------------------------------------------------------------
// The text form, which is what says how to read the text a key is supplied as
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize, Serialize, Default, Describe)]
struct Shapes {
    /// A string.
    #[serde(default)]
    name: String,
    /// A number.
    #[serde(default)]
    count: u32,
    /// A flag.
    #[serde(default)]
    on: bool,
    /// A list.
    #[serde(default)]
    repos: Vec<String>,
    /// A map.
    #[serde(default)]
    labels: std::collections::BTreeMap<String, String>,
    /// Something this crate cannot interpret.
    #[serde(default)]
    endpoint: Newtype,
}

// No `Describe`: it is a leaf, and the derive reports the token `Newtype` as its type without
// needing the type to describe anything. That is the point of the fixture — a spelling the crate
// cannot interpret.
#[derive(Deserialize, Serialize, Default)]
struct Newtype(String);

fn shapes() -> Contract {
    Terrace::new("SHAPES_")
        .schema::<Shapes>()
        .with_defaults_from(&Shapes::default())
        .expect("serialises")
        .into_contract(App::new("shapes"))
        .build()
        .expect("builds")
}

#[test]
fn every_key_says_how_to_read_its_text() {
    let contract = shapes();
    let form = |path: &str| key_of(&contract, path).text_form;

    assert_eq!(form("name"), TextForm::Text);
    assert_eq!(form("count"), TextForm::Integer);
    assert_eq!(form("on"), TextForm::Boolean);
    assert_eq!(form("repos"), TextForm::Structured);
    assert_eq!(form("labels"), TextForm::Structured);
    // Not `Text`. The distinction is the point: `Text` says any text is fine, `Unknown` says
    // nothing could be determined and a check might have paid.
    assert_eq!(form("endpoint"), TextForm::Unknown);
}

#[derive(Deserialize, Serialize, Describe)]
struct Parsing {
    /// Accepts anything.
    #[serde(default)]
    text: String,
    /// Accepts anything.
    #[serde(default)]
    path: std::path::PathBuf,
    /// Parses, and refuses.
    listen: std::net::IpAddr,
    /// Parses, and refuses.
    peer: std::net::SocketAddr,
    /// Parses, and refuses.
    marker: char,
}

#[test]
fn a_type_that_parses_its_string_is_not_unconstrained_text() {
    // Measured against the loader: given `!!!`, a `String` and a `PathBuf` key load, while an
    // `IpAddr` fails with "invalid IP address syntax", a `SocketAddr` with "invalid socket address
    // syntax" and a `char` with "expected a character".
    //
    // `Text` promises no check was needed. For the last three that is a claim the loader
    // contradicts — the same "one value, two meanings" defect `TextForm` exists to remove, one
    // field over. `Unknown` says the true thing: a check exists and this document does not
    // describe it.
    let schema = Terrace::new("PARSING_").schema::<Parsing>();
    let form = |path: &str| {
        schema
            .keys
            .iter()
            .find(|key| key.path == path)
            .unwrap_or_else(|| panic!("{path} is described"))
            .text_form
    };

    assert_eq!(form("text"), TextForm::Text);
    assert_eq!(form("path"), TextForm::Text);
    assert_eq!(form("listen"), TextForm::Unknown);
    assert_eq!(form("peer"), TextForm::Unknown);
    assert_eq!(form("marker"), TextForm::Unknown);
}

#[test]
fn a_string_constraint_is_what_says_a_file_can_supply_a_key() {
    // Measured against the loader, supplying each key as a secrets-directory file: `text`, `path`,
    // `listen`, `peer` and `marker` all load; a float and a `Vec` are refused with "invalid type:
    // found string". `constraint.type == "string"` predicts every row; `text_form: text` gets
    // `IpAddr`, `SocketAddr` and `char` wrong, because those parse a string rather than being
    // anything but one in the document.
    //
    // The two rules agreed until the parsing types were reclassified, and a gate written against
    // the wrong one turned a false accept into a false rejection of a correct deployment.
    let schema = Terrace::new("PARSING_").schema::<Parsing>();
    let is_string = |path: &str| {
        schema
            .keys
            .iter()
            .find(|key| key.path == path)
            .unwrap_or_else(|| panic!("{path} is described"))
            .constraint
            .as_ref()
            .and_then(|c| c.get("type"))
            .and_then(Json::as_str)
            == Some("string")
    };

    for path in ["text", "path", "listen", "peer", "marker"] {
        assert!(is_string(path), "{path} mounts from a secrets file");
    }
}

#[test]
fn the_document_half_still_calls_them_strings() {
    // Only the *form* claim was wrong. In document space every one of these is a TOML string, and
    // `constraint` saying so is correct — a TOML file writing `listen = "127.0.0.1"` is right.
    let schema = Terrace::new("PARSING_").schema::<Parsing>();
    for path in ["text", "path", "listen", "peer"] {
        let key = schema
            .keys
            .iter()
            .find(|key| key.path == path)
            .expect("described");
        assert_eq!(key.constraint, Some(json!({ "type": "string" })), "{path}");
    }
}

#[test]
fn a_list_key_requires_the_bracket_form_rather_than_accepting_anything() {
    // The case that made the form necessary. `text_constraint: null` used to mean both "any text
    // is fine" and "this needs a structured literal nobody described", so a chart setting
    // `SHAPES_REPOS=a,b` — the first thing anyone would try — passed every gate and failed at
    // boot with a type error.
    let contract = shapes();
    let repos = key_of(&contract, "repos");

    assert_eq!(repos.text_form, TextForm::Structured);
    let pattern = repos.text_constraint.as_ref().expect("carried")["pattern"]
        .as_str()
        .expect("a string");

    // Measured: figment takes `[]` and `["a","b"]` for a `Vec<String>` and refuses `a,b`, `a` and
    // the empty string.
    for text in ["[]", "[\"a\",\"b\"]", " [ 1, 2 ] "] {
        assert!(bracketed(pattern, text), "{text} must be admitted");
    }
    for text in ["a,b", "a", "", "["] {
        assert!(!bracketed(pattern, text), "{text} must be refused");
    }
}

#[test]
fn a_string_key_says_any_text_is_fine_rather_than_saying_nothing() {
    let contract = shapes();
    let name = key_of(&contract, "name");

    // No pattern, because an environment value is a string already — but the form is what makes
    // that "nothing to check" rather than "nothing is known".
    assert_eq!(name.text_constraint, None);
    assert_eq!(name.text_form, TextForm::Text);
}

#[test]
fn a_stated_constraint_takes_a_stated_form() {
    let contract = with_external(
        External::new().var(
            ExternalVar::new("TIMEOUT")
                .ty("Duration")
                .constraint(
                    None,
                    Some(json!({ "type": "string", "pattern": "^[0-9]+s$" })),
                )
                .text_form(TextForm::Text),
        ),
    )
    .expect("builds");

    assert_eq!(contract.external.env[0].text_form, TextForm::Text);
}

/// Whether `text` satisfies the bracket pattern this crate emits for a structured key.
///
/// Hand-rolled for `matches_pattern`'s reason: the crate takes no regex dependency, and asserting
/// four strings against one anchored shape does not justify a dev-dependency for one either.
fn bracketed(pattern: &str, text: &str) -> bool {
    assert!(
        pattern.contains("[\\[\\{]") && pattern.contains("[\\]\\}]"),
        "this matcher only understands the shape this crate emits: {pattern}"
    );
    let text = text.trim();
    let opens = text.starts_with('[') || text.starts_with('{');
    let closes = text.ends_with(']') || text.ends_with('}');
    opens && closes && text.len() >= 2
}

// ---------------------------------------------------------------------------------------------
// Forward compatibility, which the version policy promises and only a fallback variant delivers
// ---------------------------------------------------------------------------------------------

#[test]
fn a_document_from_a_later_crate_still_reads() {
    // `CONTRACT_VERSION` promises a version is bumped only when a field changes meaning or
    // disappears, never when one is added — so a consumer must be able to read a document carrying
    // things it has no name for. A consumer branching on strings gets that for free; one
    // deserialising into typed enums does not, and an unfamiliar variant is a parse error for the
    // *whole document* rather than for the field carrying it.
    let rendered = contract().to_json().expect("renders");
    let mut json: Json = serde_json::from_str(&rendered).expect("parses");

    // A field the envelope gained, a field a key gained, and one of each enum set to a spelling
    // this version has never heard of.
    json["future_envelope_field"] = json!("ignored");
    let keys = json["schema"]["keys"].as_array_mut().expect("an array");
    keys[0]["future_key_field"] = json!("ignored");
    keys[0]["text_form"] = json!("duration");
    json["schema"]["loader"][0]["role"] = json!("overlay");
    json["external"]["unknown"] = json!("report");

    let parsed: Contract =
        serde_json::from_value(json).expect("a later crate's document is still readable");

    // Each fallback is the one that keeps a consumer honest rather than the one that is nearest.
    assert_eq!(parsed.schema.keys[0].text_form, TextForm::Unknown);
    assert_eq!(parsed.schema.loader[0].role, LoaderRole::Other);
    // Failing closed: read as `Allow` this would silently switch off the ordered list's last step.
    assert_eq!(parsed.external.unknown, Unknown::Reject);

    // And the rest of the document survived, which is the whole point — one unfamiliar form on one
    // key used to make the envelope, the app block and every other key unreadable.
    assert_eq!(parsed.terrace_contract, CONTRACT_VERSION);
    assert_eq!(parsed.app.name, "portfolio");
    assert!(parsed.json_schema["properties"].is_object());
    assert!(parsed.schema.keys.len() > 1);
}

#[test]
fn an_unknown_role_is_not_read_as_a_role_that_means_something() {
    // The reason `LoaderRole` needed a new variant rather than folding into one of the three. Every
    // role means "the loader reads this variable", so the ordered list's step 1 is satisfied by
    // `env` alone whatever this says — but a consumer looking for the secrets directory matches on
    // the role, and an unknown one read as `Config` would hand it the wrong variable.
    let role: LoaderRole = serde_json::from_str("\"overlay\"").expect("reads");

    assert_eq!(role, LoaderRole::Other);
    assert_ne!(role, LoaderRole::Config);
    assert_ne!(role, LoaderRole::SecretsDir);
}

#[test]
fn the_known_spellings_still_round_trip() {
    // A fallback that swallowed a spelling this version does know would be worse than none.
    for (json, form) in [
        ("\"text\"", TextForm::Text),
        ("\"integer\"", TextForm::Integer),
        ("\"boolean\"", TextForm::Boolean),
        ("\"choice\"", TextForm::Choice),
        ("\"structured\"", TextForm::Structured),
        ("\"unknown\"", TextForm::Unknown),
    ] {
        assert_eq!(
            serde_json::from_str::<TextForm>(json).expect("reads"),
            form,
            "{json}"
        );
        assert_eq!(serde_json::to_string(&form).expect("renders"), json);
    }

    for (json, policy) in [
        ("\"reject\"", Unknown::Reject),
        ("\"warn\"", Unknown::Warn),
        ("\"allow\"", Unknown::Allow),
    ] {
        assert_eq!(
            serde_json::from_str::<Unknown>(json).expect("reads"),
            policy,
            "{json}"
        );
        assert_eq!(serde_json::to_string(&policy).expect("renders"), json);
    }
}

// ---------------------------------------------------------------------------------------------
// The element shape of a container-typed key, which is the half a chart used to transcribe
// ---------------------------------------------------------------------------------------------

/// One element of `routes`, with a nested table and a choice inside it — the shape a chart's
/// `values.schema.json` had to carry by hand while the contract said only `{"type": "array"}`.
#[derive(Deserialize, Serialize, Default, Describe)]
struct Route {
    /// Name shown in the log line.
    name: String,
    /// Lowest severity this route accepts.
    #[config(values)]
    #[serde(default)]
    min_severity: Severity,
    /// Where the route delivers.
    #[config(nested)]
    target: Target,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Target {
    /// Channel the card is posted to.
    #[serde(default)]
    id: u16,
}

#[derive(Deserialize, Serialize, Default, Describe)]
#[serde(rename_all = "lowercase")]
enum Severity {
    #[default]
    Info,
    Critical,
}

/// Deliberately not `Describe`: the keys under it are operator-chosen names. A producer that has
/// said so must keep publishing exactly what it published before.
#[derive(Deserialize, Serialize, Default)]
struct Bucket {
    region: String,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Routed {
    /// Routes declared in the file.
    #[config(element)]
    #[serde(default)]
    routes: Vec<Route>,
    /// Buckets, by the route name an operator chose.
    #[serde(default)]
    entries: BTreeMap<String, Bucket>,
}

fn routed() -> Contract {
    terrace()
        .schema::<Routed>()
        .with_defaults_from(&Routed::default())
        .expect("the default config serialises")
        .into_contract(App::new("router").version("v1.0.0"))
        .build()
        .expect("the contract has nothing to refuse")
}

/// The document a deployment pipeline acts on now carries the element, so the pipeline stops
/// carrying a copy of the struct that goes stale the moment the image moves.
#[test]
fn a_container_typed_key_publishes_its_element_shape() {
    let contract = routed();
    let routes = key_of(&contract, "routes")
        .constraint
        .clone()
        .expect("an array of described routes");

    assert_eq!(routes["type"], json!("array"));
    assert_eq!(
        routes["items"]["properties"]["name"]["type"],
        json!("string")
    );
    assert_eq!(
        routes["items"]["properties"]["min_severity"]["enum"],
        json!(["info", "critical"])
    );
    assert_eq!(
        routes["items"]["properties"]["target"]["properties"]["id"]["maximum"],
        json!(65_535)
    );
}

/// The element is a nested schema on one key, never keys of its own. A consumer walking the key
/// list — which is how every environment-variable gate is written — sees exactly what it saw.
#[test]
fn an_element_contributes_no_keys_to_the_contract() {
    let paths: Vec<String> = routed()
        .schema
        .keys
        .iter()
        .map(|key| key.path.clone())
        .collect();

    assert_eq!(paths, ["routes", "entries"]);
}

/// Opt-in. A container whose element type is deliberately a leaf publishes the bytes it always
/// did, so re-vendoring a contract from a producer that did nothing changes nothing.
#[test]
fn a_container_whose_element_says_nothing_is_unchanged() {
    assert_eq!(
        key_of(&routed(), "entries").constraint,
        Some(json!({ "type": "object" }))
    );
}

/// The element lives in document space. An environment variable still carries the whole container
/// as one TOML literal, and `text_form` is what tells a consumer to read it that way.
#[test]
fn an_element_shape_does_not_reach_the_text_constraint() {
    let contract = routed();
    let routes = key_of(&contract, "routes");

    assert_eq!(routes.text_form, TextForm::Structured);
    assert_eq!(
        routes.text_constraint,
        key_of(&contract, "entries").text_constraint
    );
}

/// Two renderings of one fact, and the element is the case where they could most easily part:
/// the flat field is left open and the rendered document closes what it renders.
#[test]
fn the_nested_element_is_the_flat_one_with_the_documents_own_strictness() {
    let contract = routed();
    let nested = &contract.json_schema["properties"]["routes"];
    let flat = key_of(&contract, "routes")
        .constraint
        .clone()
        .expect("carried");

    assert_eq!(nested["type"], flat["type"]);
    for field in ["name", "min_severity"] {
        assert_eq!(
            nested["items"]["properties"][field], flat["items"]["properties"][field],
            "{field}"
        );
    }
    assert_eq!(
        nested["items"]["properties"]["target"]["properties"],
        flat["items"]["properties"]["target"]["properties"]
    );

    // The one deliberate difference, at every level of the element. `serde` accepts a field
    // nobody declared unless the struct says otherwise, so the flat constraint — the
    // certainly-true half — leaves the element open, and `JsonSchema::closed` is what decides it
    // for a rendering.
    for at in [&nested["items"], &nested["items"]["properties"]["target"]] {
        assert_eq!(at["additionalProperties"], json!(false), "{at}");
    }
    for at in [&flat["items"], &flat["items"]["properties"]["target"]] {
        assert!(at.get("additionalProperties").is_none(), "{at}");
    }
}
