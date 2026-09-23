//! What the contract says about reloading, and the claims about it that it refuses to publish.
//!
//! A chart leaves every `live` key out of the digest that rolls its pods, so a key published
//! `live` that the process does not apply on a rebuild is a change a deployment reports as done
//! and never makes. The tests are about the two ways that could happen: a class the author did
//! not write, and a class the rest of the same document contradicts.

#![cfg(feature = "schema")]
#![expect(dead_code, reason = "fixtures are read by the derive, not at runtime")]

use serde::Deserialize;
use serde_json::{Value as Json, json};
use terrace_config::Terrace;
use terrace_config::schema::{App, Describe, Reload, ReloadMode, ReloadSupport, Schema};

#[derive(Deserialize, Describe)]
#[config(reload = "live")]
struct Config {
    /// Applied by the next rebuild.
    #[serde(default)]
    ttl_secs: u64,
    /// Read before the supervisor starts.
    #[config(reload = "restart")]
    #[serde(default)]
    log_level: String,
    #[config(nested)]
    storage: Storage,
    #[config(nested)]
    #[serde(default)]
    http: Http,
}

/// A type that is `restart` wherever it is used.
#[derive(Deserialize, Describe)]
#[config(reload = "restart")]
struct Storage {
    /// Where the data lives.
    backend: String,
}

/// A type that says nothing, and so inherits.
#[derive(Deserialize, Default, Describe)]
struct Http {
    /// The listener's port.
    #[serde(default)]
    port: u16,
}

/// A root that says nothing at all.
#[derive(Deserialize, Describe)]
struct Unannotated {
    /// Applied, or not; nobody said.
    #[serde(default)]
    ttl_secs: u64,
}

/// A field marked `restart` holding a type that calls itself `live`.
#[derive(Deserialize, Describe)]
struct Contradicting {
    #[config(nested)]
    #[config(reload = "restart")]
    inner: LiveInner,
}

#[derive(Deserialize, Describe)]
#[config(reload = "live")]
struct LiveInner {
    /// Live in its own type, restart where it is held.
    #[serde(default)]
    value: u64,
}

fn classes(schema: &Schema) -> Vec<(&str, Option<Reload>)> {
    schema
        .keys
        .iter()
        .map(|key| (key.path.as_str(), key.reload))
        .collect()
}

fn rebuilding() -> Terrace {
    Terrace::new("TEST_")
        .reserve("TEST_PROFILE")
        .reloads(ReloadSupport::rebuild())
}

#[test]
fn restart_wins_wherever_it_is_written() {
    let schema = rebuilding().schema::<Config>();
    assert_eq!(
        classes(&schema),
        [
            ("ttl_secs", Some(Reload::Live)),
            ("log_level", Some(Reload::Restart)),
            ("storage.backend", Some(Reload::Restart)),
            ("http.port", Some(Reload::Live)),
        ]
    );

    let contradicting = rebuilding().schema::<Contradicting>();
    assert_eq!(
        classes(&contradicting),
        [("inner.value", Some(Reload::Restart))],
        "a field's `restart` is not overridden by the `live` its type declares"
    );
}

#[test]
fn a_key_nobody_marked_is_published_restart_once_the_binary_declares() {
    let undeclared = Terrace::new("TEST_").schema::<Unannotated>();
    assert_eq!(classes(&undeclared), [("ttl_secs", None)]);
    assert!(undeclared.reload.is_none());

    let declared = rebuilding().schema::<Unannotated>();
    assert_eq!(classes(&declared), [("ttl_secs", Some(Reload::Restart))]);
}

#[test]
fn a_binary_that_does_not_rebuild_publishes_every_key_restart() {
    let schema = Terrace::new("TEST_")
        .reloads(ReloadSupport::none())
        .schema::<Config>();
    assert!(
        classes(&schema)
            .iter()
            .all(|(_, class)| *class == Some(Reload::Restart))
    );
    assert_eq!(
        schema.reload.as_ref().map(|s| s.mode),
        Some(ReloadMode::None)
    );
    schema
        .into_contract(App::new("test"))
        .build()
        .expect("a `none` binary with `live`-annotated types builds");
}

#[test]
fn nothing_is_published_until_the_binary_declares() {
    let json: Json = serde_json::from_str(
        &Terrace::new("TEST_")
            .schema::<Unannotated>()
            .to_json()
            .expect("json"),
    )
    .expect("parses");
    assert!(json.get("reload").is_none());
    assert!(json["keys"][0].get("reload").is_none());
}

#[test]
fn the_declaration_is_published_beside_the_keys() {
    let json: Json =
        serde_json::from_str(&rebuilding().schema::<Config>().to_json().expect("json"))
            .expect("parses");
    assert_eq!(
        json["reload"],
        json!({ "mode": "rebuild", "layers": ["document", "secrets_dir", "env_file"] })
    );
    assert_eq!(json["keys"][0]["reload"], "live");
}

#[test]
fn a_live_key_in_a_binary_that_declared_nothing_is_refused() {
    let error = Terrace::new("TEST_")
        .schema::<Config>()
        .into_contract(App::new("test"))
        .build()
        .expect_err("refusal 10");
    assert!(error.to_string().contains("ttl_secs"), "{error}");
    assert!(error.to_string().contains("Terrace::reloads"), "{error}");
}

#[test]
fn a_hand_built_contradiction_is_refused() {
    let mut schema = rebuilding().schema::<Config>();
    schema.reload = Some(ReloadSupport::none());
    let error = schema
        .into_contract(App::new("test"))
        .build()
        .expect_err("refusal 10");
    assert!(error.to_string().contains("declares no rebuild"), "{error}");

    let mut schema = rebuilding().schema::<Config>();
    let mut support = ReloadSupport::rebuild();
    support.layers.clear();
    schema.reload = Some(support);
    let error = schema
        .into_contract(App::new("test"))
        .build()
        .expect_err("refusal 12");
    assert!(error.to_string().contains("watches no layer"), "{error}");
}

#[test]
fn a_reserved_key_is_restart_whatever_it_was_annotated() {
    #[derive(Deserialize, Describe)]
    #[config(reload = "live")]
    struct WithProfile {
        /// Read from the environment before the layers exist.
        profile: String,
    }
    let schema = rebuilding().schema::<WithProfile>();
    assert!(schema.keys[0].reserved);
    assert_eq!(schema.keys[0].reload, Some(Reload::Restart));

    let mut forced = schema;
    forced.keys[0].reload = Some(Reload::Live);
    let error = forced
        .into_contract(App::new("test"))
        .build()
        .expect_err("refusal 11");
    assert!(error.to_string().contains("reserved"), "{error}");
}

#[test]
fn restart_paths_include_every_alias() {
    #[derive(Deserialize, Describe)]
    struct Aliased {
        /// Renamed once.
        #[serde(alias = "old_name")]
        #[config(reload = "restart")]
        new_name: String,
    }
    let schema = rebuilding().schema::<Aliased>();
    assert_eq!(schema.restart_paths(), ["new_name", "old_name"]);
}
