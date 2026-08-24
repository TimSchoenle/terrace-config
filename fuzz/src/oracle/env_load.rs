//! Fuzzes the environment layer — `Terrace::load` and `Terrace::load_watched` over arbitrary
//! `TEST_*` variables.
//!
//! The environment is not attacker-controlled the way a mounted secret is, so the threat model
//! is a typo: figment ignores an unknown key rather than rejecting it, so this oracle looks for
//! the worse case — a correctly-spelled key whose value aborts a service at startup instead of
//! producing a typed error.
//!
//! # Oracle
//! 1. **Totality.** `Ok` or [`terrace_config::Error`], never a panic. Deep `__` nesting is the
//!    interesting shape: key paths are split into segments and rebuilt through a recursive
//!    insert, so `TEST_A__B__C__…` is a stack-depth question the seeds ask directly.
//! 2. **Boot and reload see the same configuration.** `load` and `load_watched` must agree on
//!    success, and on success the extracted value must equal `load_watched`'s fingerprint. A
//!    divergence means a reload could rebuild a service with values a fresh boot would never
//!    produce — and nothing about the happy path would look different.
//! 3. **An unchanged environment is not a change.** Two consecutive `load_watched` calls must
//!    report `differs_from == false`. This is the no-op detection the supervisor relies on to
//!    avoid rebinding a listener for a `..data` swap that moved no key.
//!
//! # Input shape
//! `e:<SUFFIX>=<VALUE>` per line, with the `TEST_` prefix supplied here so seeds stay readable
//! and mutation time is not spent on a prefix `Env::prefixed` discards.
//!
//! The keys that make a load read the filesystem are filtered out — `TEST_CONFIG`,
//! `TEST_SECRETS_DIR` and anything ending `_FILE`. A reproducer whose behaviour depends on a
//! file elsewhere on the machine is not a reproducer. Those layers have oracles of their own,
//! which build the directory they read.

use figment::value::Value;
use secrecy::SecretString;
use serde::Deserialize;
use terrace_config::Terrace;
use terrace_config::testing::Harness;

use crate::support::{Directive, PREFIX, directives, is_safe_env};

/// A shape with the two properties that have bitten this loader: a nested `SecretString` leaf,
/// which an all-digit value must still deserialise into, and numeric fields, which are where a
/// range error must surface as a typed failure rather than a panic.
#[derive(Debug, Deserialize)]
#[expect(
    dead_code,
    reason = "the deserializer is the consumer — the parse is the code under test, and reading \
              the fields back would prove nothing the extraction has not already proven"
)]
struct Sample {
    database: Database,
    #[serde(default)]
    limits: Limits,
}

#[derive(Debug, Deserialize)]
#[expect(dead_code, reason = "as `Sample` above")]
struct Database {
    url: SecretString,
    #[serde(default)]
    max_connections: u32,
}

#[derive(Debug, Deserialize, Default)]
#[expect(dead_code, reason = "as `Sample` above")]
struct Limits {
    #[serde(default)]
    rate: f32,
    #[serde(default)]
    burst: u16,
}

/// A minimally bootable environment, applied before the fuzzer's pairs so they can override it.
///
/// Without this most inputs fail on the first missing required field before reaching the value
/// parsing this oracle is about.
const BASE_ENV: &[(&str, &str)] = &[("TEST_DATABASE__URL", "postgres://u:p@localhost/db")];

fn layers() -> Terrace {
    Terrace::new(PREFIX).reserve("TEST_PROFILE")
}

/// The sandbox each iteration runs in: an empty environment, restored afterwards, around the loader
/// under test.
///
/// `std::env::set_var` is `unsafe` in edition 2024 and both crates forbid unsafe code, so a jail is
/// not a convenience here — it is the only way in.
fn harness() -> Harness {
    Harness::over(layers())
}

/// Run the oracle.
///
/// Panics when the loader breaks one of the rules above.
///
/// # Panics
/// That is the contract: a panic is the finding.
pub fn check(data: &str) {
    // Ignored: a jail-setup failure (a temp dir it could not create) says nothing about the code
    // under test. Every outcome that does is handled inside the closure.
    let _ = harness().try_run(|jail| {
        for (key, value) in BASE_ENV {
            jail.env(key, value);
        }

        for directive in directives(data) {
            let Directive::Env { suffix, value } = directive else {
                continue;
            };
            if !is_safe_env(suffix, value) {
                continue;
            }
            let name = format!("{PREFIX}{suffix}");
            // See the module doc: every layer that reads the filesystem is deliberately out of
            // reach, so a crash always reproduces from the input alone.
            //
            // Case-insensitively, because Windows environment names are: `TEST_secrets_dir` and
            // `TEST_SECRETS_DIR` are one variable there, and an exact-match filter would let the
            // input point the loader at a path on the host after all.
            if name.eq_ignore_ascii_case("TEST_CONFIG")
                || name.eq_ignore_ascii_case("TEST_SECRETS_DIR")
                || name.to_ascii_uppercase().ends_with("_FILE")
            {
                continue;
            }
            jail.env(&name, value);
        }

        // (1) Totality, through the typed extraction where a second round of interpretation
        // happens. Either outcome is legitimate for almost any input.
        let _ = jail.load::<Sample>();

        // (2) Boot and reload must see the same thing.
        let direct = jail.load::<Value>();
        let watched = jail.load_watched::<Value>();
        match (direct, watched) {
            (Ok(direct), Ok(watched)) => {
                assert_eq!(
                    direct, watched.value,
                    "load and load_watched extracted different values from one environment"
                );

                // (3) Re-reading an environment nothing touched is not a change.
                let again = jail
                    .load_watched::<Value>()
                    .expect("a load that just succeeded must succeed again unchanged");
                assert!(
                    !again.sources.differs_from(&watched.sources),
                    "an unchanged environment reported a change"
                );
            }
            (Err(_), Err(_)) => {}
            (direct, watched) => panic!(
                "load and load_watched disagreed on success: load {}, load_watched {}",
                if direct.is_ok() { "ok" } else { "err" },
                if watched.is_ok() { "ok" } else { "err" },
            ),
        }

        Ok(())
    });
}
