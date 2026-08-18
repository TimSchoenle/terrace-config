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
use serde_json::{Value as Json, json};
use terrace_config::Terrace;
use terrace_config::schema::{
    App, CONTRACT_VERSION, Contract, DEFAULT_PATH, DRAFT_07, Describe, External, ExternalVar,
    JsonSchema, LABEL_PATH, LABEL_PREFIX, LABEL_VERSION, Schema, Unknown,
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
    // be written after the push — changing the bytes `LABEL_SHA256` was computed over before it,
    // which §3.3 of the plan defines as a hard error. The tie is the attachment: whatever comes
    // back from asking a digest for its `ARTIFACT_TYPE` referrers is that digest's contract.
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
        .json_schema(JsonSchema::new().meta_schema(DRAFT_07).title("chosen"))
        .build()
        .expect("builds");
    assert_eq!(titled.json_schema["title"], Json::from("chosen"));
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

#[test]
fn a_choice_repeats_its_spellings_in_text_space() {
    // Not redundant with `constraint`: a consumer reading an absent text constraint as
    // "unconstrained" would otherwise lose the check entirely on the one layer where every value
    // is text.
    let contract =
        with_external(External::new().var(ExternalVar::new("MODE").values(["fast", "slow"])))
            .expect("builds");

    assert_eq!(
        contract.external.env[0].text_constraint,
        Some(json!({ "type": "string", "enum": ["fast", "slow"] }))
    );
}

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
