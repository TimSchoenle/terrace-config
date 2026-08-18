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
use terrace_config::schema::{App, Describe, External, ExternalVar, Key, Unreachable};
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

// ---------------------------------------------------------------------------------------------
// The same shapes again, with an alias — the spelling every rule above was written without
// ---------------------------------------------------------------------------------------------

/// A `rename_all` container whose field keeps the old spelling working. The migration
/// `env_aliases` exists for, applied to the shape that has no canonical spelling.
#[derive(Deserialize, Serialize, Default, Describe)]
#[serde(rename_all = "camelCase")]
struct CamelWithAlias {
    /// A key the environment cannot name — under that name.
    #[serde(default, alias = "cache_dir")]
    cache_dir: String,
}

#[test]
fn a_working_alias_means_the_key_is_reachable() {
    // `unreachable` is a fact about the *key*, and a key answers to every one of its spellings.
    // Read off the canonical spelling alone it said the environment could not reach a key that a
    // working alias reaches — worse than the bare `None` it replaced, because a consumer believing
    // it tells an operator a working configuration is impossible.
    let key = key_of(
        &Terrace::new("PROBE_").schema::<CamelWithAlias>(),
        "cacheDir",
    );

    assert_eq!(key.env, None);
    assert_eq!(key.env_aliases, ["PROBE_CACHE_DIR"]);
    assert_eq!(key.secrets_file_aliases, ["cache_dir"]);
    assert_eq!(key.unreachable, None);
}

#[test]
fn and_the_alias_really_does_reach_it() {
    let mut loaded = String::new();
    Harness::over(Terrace::new("PROBE_")).run(|jail| {
        jail.env("PROBE_CACHE_DIR", "from-the-environment");
        loaded = jail.load::<CamelWithAlias>().expect("loads").cache_dir;
        Ok(())
    });

    assert_eq!(loaded, "from-the-environment");
}

/// The indirection collision reached through an alias rather than through a field name.
#[derive(Deserialize, Serialize, Default, Describe)]
struct AliasedIndirection {
    /// The key that claims `<PREFIX>TOKEN_FILE`.
    #[serde(default)]
    token: String,
    /// A key whose *alias* is the name that variable takes.
    #[serde(default, alias = "token_file")]
    path: String,
}

#[test]
fn an_indirection_collision_through_an_alias_is_refused_too() {
    // Identical in effect to the canonical case, down to the values — one variable, two keys — and
    // it built, because the check read canonical spellings only. `Indirection` is reported on any
    // spelling that collides, even beside one that works, because the hazard does not go away when
    // another spelling does.
    let schema = Terrace::new("PROBE_").schema::<AliasedIndirection>();
    let aliased = key_of(&schema, "path");

    assert_eq!(aliased.env.as_deref(), Some("PROBE_PATH"));
    assert_eq!(aliased.unreachable, Some(Unreachable::Indirection));

    let mut both = (String::new(), false);
    Harness::over(Terrace::new("PROBE_")).run(|jail| {
        let held = jail.write("held", "s3cret")?;
        jail.env("PROBE_TOKEN_FILE", held.display().to_string());
        let loaded = jail.load::<AliasedIndirection>().expect("loads");
        both = (loaded.token, !loaded.path.is_empty());
        Ok(())
    });
    assert_eq!(both.0, "s3cret");
    assert!(both.1, "the same variable filled the aliased key");

    let error = Terrace::new("PROBE_")
        .schema::<AliasedIndirection>()
        .with_defaults_from(&AliasedIndirection::default())
        .expect("serialises")
        .into_contract(App::new("collide"))
        .build()
        .expect_err("refused");
    assert!(error.to_string().contains("path"), "{error}");
}

#[test]
fn an_external_variable_may_not_take_an_alias_spelling_either() {
    // The third door the round named: an alias spelling is one the loader answers to, so declaring
    // it external — or covering it with an ignore pattern — exempts a real key from the check that
    // owns it, exactly as the canonical spelling would.
    let schema = || {
        Terrace::new("PROBE_")
            .schema::<CamelWithAlias>()
            .with_defaults_from(&CamelWithAlias::default())
            .expect("serialises")
    };

    let declared = schema()
        .into_contract(App::new("aliased"))
        .external(External::new().var(ExternalVar::new("PROBE_CACHE_DIR")))
        .build()
        .expect_err("refused");
    assert!(
        declared.to_string().contains("PROBE_CACHE_DIR"),
        "{declared}"
    );

    let ignored = schema()
        .into_contract(App::new("aliased"))
        .external(External::new().ignore("PROBE_CACHE_DIR"))
        .build()
        .expect_err("refused");
    assert!(ignored.to_string().contains("PROBE_CACHE_DIR"), "{ignored}");
}
