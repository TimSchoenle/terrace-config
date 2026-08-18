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
use serde_json::Value as Json;
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

    assert_eq!(json["terraceContract"], Json::from(CONTRACT_VERSION));
    assert_eq!(json["app"]["name"], Json::from("portfolio"));
    assert_eq!(json["app"]["version"], Json::from("v2.5.0"));
    // The schema half is what lets a validator check an environment variable and a secrets file
    // name; the JSON Schema half is the only one a stock validator can act on. Publishing one
    // without the other is what the envelope exists to prevent.
    assert!(json["schema"]["keys"].is_array());
    assert!(json["jsonSchema"]["properties"].is_object());
    assert_eq!(json["external"]["unknown"], Json::from("reject"));
}

#[test]
fn a_field_nothing_was_said_about_is_absent_rather_than_null() {
    let json: Json = serde_json::from_str(&contract().to_json().expect("renders")).expect("parses");

    // `digest` is filled in after the push, by whatever attaches the artifact. A `null` here would
    // be indistinguishable from a build that tried and failed to record one.
    assert!(json["app"].get("digest").is_none());
    assert!(json["app"].get("revision").is_none());
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
fn marking_an_external_variable_secret_drops_its_default() {
    let var = ExternalVar::new("API_TOKEN").default("hunter2").secret();

    assert!(var.secret);
    assert_eq!(var.default, None);
}

#[test]
fn an_external_secret_that_kept_a_default_is_refused() {
    let mut var = ExternalVar::new("API_TOKEN").secret();
    var.default = Some("hunter2".to_owned());

    let error = with_external(External::new().var(var)).expect_err("refused");
    assert!(error.to_string().contains("API_TOKEN"));
    assert!(!error.to_string().contains("hunter2"));
}
