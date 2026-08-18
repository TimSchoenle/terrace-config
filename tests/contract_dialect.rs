//! Every way a key can lose a spelling, checked against what the loader answers to.
//!
//! `contract_read.rs` walks every [`TextForm`](terrace_config::schema::TextForm) because a test was
//! written to force that. Nothing walked the *dialect* the same way, and four findings in one round
//! came out of shapes the worked example does not have: a field name ending in the indirection
//! suffix, a path carrying the nesting separator, a `rename_all` container, an empty prefix.
//!
//! The property under test is the one a consumer relies on and cannot verify: **a spelling this
//! crate publishes is one the loader answers to, and a spelling it withholds is one the loader
//! does not** — except for the single case where withholding is not the same as unreachable, which
//! is why that case is refused rather than published.

#![cfg(all(feature = "schema", feature = "testing"))]

use serde::{Deserialize, Serialize};
use terrace_config::Terrace;
use terrace_config::schema::{App, Describe, Key, Unreachable};
use terrace_config::testing::Harness;

/// A name ending in the indirection suffix, beside the key that claims it.
#[derive(Deserialize, Serialize, Default, Describe)]
struct Indirection {
    /// The key that claims `<PREFIX>TOKEN_FILE`.
    #[serde(default)]
    token: String,
    /// The key whose environment spelling that would have been.
    #[serde(default)]
    token_file: String,
}

/// A container whose renaming makes every key unspellable in the environment.
#[derive(Deserialize, Serialize, Default, Describe)]
#[serde(rename_all = "camelCase")]
struct Camel {
    /// A key the environment cannot name.
    #[serde(default)]
    cache_dir: String,
}

/// A leaf whose own name carries the nesting separator.
#[derive(Deserialize, Serialize, Default, Describe)]
struct Separated {
    /// A key whose path is ambiguous once spelled.
    #[serde(default, rename = "b__c")]
    b_c: String,
}

fn key_of(schema: &terrace_config::schema::Schema, path: &str) -> Key {
    schema
        .keys
        .iter()
        .find(|key| key.path == path)
        .unwrap_or_else(|| panic!("no key is described at that path"))
        .clone()
}

#[test]
fn a_renamed_container_reports_why_it_has_no_spelling() {
    // Measured: neither `PROBE_CACHEDIR` nor `PROBE_CACHE_DIR` reaches the key, and neither does a
    // secrets file named `cacheDir` or `cache_dir`. An environment key is folded to lower case on
    // the way in and `cacheDir` never comes back.
    let key = key_of(&Terrace::new("PROBE_").schema::<Camel>(), "cacheDir");

    assert_eq!(key.env, None);
    assert_eq!(key.env_file, None);
    assert_eq!(key.secrets_file, None);
    assert_eq!(key.unreachable, Some(Unreachable::Unnameable));
}

#[test]
fn a_path_carrying_the_separator_reports_the_same() {
    let key = key_of(&Terrace::new("PROBE_").schema::<Separated>(), "b__c");

    assert_eq!(key.env, None);
    assert_eq!(key.unreachable, Some(Unreachable::Unnameable));
}

#[test]
fn nothing_the_environment_cannot_name_is_published_as_nameable() {
    // The half a consumer relies on: a published spelling loads, and the key with none does not
    // load from the name it would have had.
    let mut camel_loaded = String::new();
    Harness::over(Terrace::new("PROBE_")).run(|jail| {
        jail.env("PROBE_CACHEDIR", "from-the-environment");
        jail.env("PROBE_CACHE_DIR", "from-the-environment");
        camel_loaded = jail.load::<Camel>().expect("loads").cache_dir;
        Ok(())
    });
    assert_eq!(
        camel_loaded, "",
        "the environment reached a key the schema says it cannot name"
    );
}

#[test]
fn a_name_that_is_another_keys_indirection_is_reported_as_reachable_elsewhere() {
    // The one case where a missing `env` does *not* mean the environment cannot reach the key —
    // which is why it gets its own reason rather than sharing `Unnameable`.
    let schema = Terrace::new("PROBE_").schema::<Indirection>();

    let claimant = key_of(&schema, "token");
    assert_eq!(claimant.env.as_deref(), Some("PROBE_TOKEN"));
    assert_eq!(claimant.env_file.as_deref(), Some("PROBE_TOKEN_FILE"));

    let loser = key_of(&schema, "token_file");
    assert_eq!(loser.env, None);
    assert_eq!(loser.unreachable, Some(Unreachable::Indirection));
    // Still nameable by a file, unlike the `Unnameable` shapes above. The two reasons differ in
    // exactly this way and a consumer reading a bare `null` could not tell them apart.
    assert_eq!(loser.secrets_file.as_deref(), Some("token_file"));
}

#[test]
fn one_variable_supplying_two_keys_is_refused_rather_than_published() {
    // Measured: `PROBE_TOKEN_FILE=<a path>` fills `token` from that file *and* fills `token_file`
    // with the path. A validator classifying that variable stops at `token`'s `env_file` and never
    // learns of the second effect, so every gate passes on a chart that is supplying a key it did
    // not mean to.
    let mut both = (String::new(), false);
    Harness::over(Terrace::new("PROBE_")).run(|jail| {
        let held = jail.write("held", "s3cret")?;
        jail.env("PROBE_TOKEN_FILE", held.display().to_string());
        let loaded = jail.load::<Indirection>().expect("loads");
        both = (loaded.token, !loaded.token_file.is_empty());
        Ok(())
    });
    assert_eq!(both.0, "s3cret", "the indirection filled the claimant");
    assert!(both.1, "and the same variable filled the other key");

    // So the contract refuses to be built rather than describing one of the two effects.
    let error = Terrace::new("PROBE_")
        .schema::<Indirection>()
        .with_defaults_from(&Indirection::default())
        .expect("serialises")
        .into_contract(App::new("collide"))
        .build()
        .expect_err("refused");

    assert!(error.to_string().contains("token_file"), "{error}");
    assert!(error.to_string().contains("Rename"), "{error}");
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Plain {
    /// A key.
    #[serde(default)]
    port: u16,
}

#[test]
fn an_empty_prefix_is_refused() {
    // The ordered list's step 4 rejects a variable that begins with the prefix and matched nothing
    // earlier — and every name begins with the empty string, so step 5 and step 6 would never be
    // reached and a declared external surface would never be read. The deeper reason is that a
    // prefixless loader cannot tell its own namespace from the machine's.
    let error = Terrace::new("")
        .schema::<Plain>()
        .with_defaults_from(&Plain::default())
        .expect("serialises")
        .into_contract(App::new("prefixless"))
        .build()
        .expect_err("refused");

    assert!(error.to_string().contains("empty prefix"), "{error}");
}

#[test]
fn an_ordinary_dialect_still_builds() {
    // The mirror-image mistake would be a rule so broad it refuses correct contracts.
    Terrace::new("PROBE_")
        .reserve("PROBE_PROFILE")
        .schema::<Plain>()
        .with_defaults_from(&Plain::default())
        .expect("serialises")
        .into_contract(App::new("ordinary"))
        .build()
        .expect("builds");
}
