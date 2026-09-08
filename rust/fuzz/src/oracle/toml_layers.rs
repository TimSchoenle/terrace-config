//! Fuzzes the TOML layer: `$TEST_CONFIG` naming a directory of fragments, merged in name order.
//!
//! The fragments are the part of a deployment an operator edits by hand, and the expansion rules
//! around them — sort by name, skip dot-prefixed entries, skip anything that is not `*.toml` —
//! are what make a `ConfigMap` mount work. A bug in any of them is silent: the layer comes back
//! smaller than it should and the service boots on defaults.
//!
//! # Oracle
//! 1. **Totality.** `Ok` or [`terrace_config::Error`], never a panic — including over TOML the
//!    fuzzer has corrupted, which must be a typed error rather than an unwind out of the parser.
//! 2. **The skip contract.** Entries a `ConfigMap` volume really does put beside the fragments
//!    are planted every iteration, each carrying valid TOML defining a sentinel key. None may
//!    ever reach the configuration.
//! 3. **Merge order.** Fragments merge sorted by name, later winning. `00-first.toml` and
//!    `zz-last.toml` define the same key and the later must win — a reversal would make a
//!    `10-base` / `20-overrides` pair silently do the opposite of what an operator reading the
//!    mount predicts.
//! 4. **Determinism, and the watch set.** Re-reading a directory nothing touched is not a
//!    change, and the directory itself is watched — the two things the supervisor needs in order
//!    to notice a rollout without rebinding a listener for nothing.
//!
//! # Input shape
//! `t:<name>=<content>` per line, with `\n` decoded so a fragment can have more than one line.
//! Only the directory form is fuzzed; the single-file form is the same expansion with the loop
//! not entered.

use figment::value::Value;
use terrace_config::Terrace;
use terrace_config::testing::Harness;

use crate::support::{Directive, PREFIX, contains_key, directives, lookup, write_file};

/// Planted every iteration under names the expansion must refuse.
///
/// `..data` is what a projected `ConfigMap` volume puts beside the real fragments; `notes.md` is
/// the README an operator drops into the same directory.
const SKIPPED: &[&str] = &["..data", "notes.md", ".hidden.toml"];

/// Valid TOML, so a leak shows up as a key rather than as a parse error.
const SKIPPED_BODY: &str = "[sentinel]\nleaked = true\n";

/// The key [`SKIPPED_BODY`] would produce if any skip stopped working.
const SENTINEL_KEY: &str = "sentinel";

/// Planted to pin merge order.
///
/// Sorted by name, `zz-last.toml` is the later of the two.
const ORDER_FIRST: &str = "00-first.toml";
const ORDER_LAST: &str = "zz-last.toml";
const ORDER_KEY: &str = "order.winner";

fn layers() -> Terrace {
    Terrace::new(PREFIX).reserve("TEST_PROFILE")
}

/// The sandbox each iteration runs in, around the loader under test.
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
    // Ignored: a jail-setup failure says nothing about the code under test.
    let _ = harness().try_run(|jail| {
        let Ok(conf) = jail.create_dir("conf.d") else {
            return Ok(());
        };
        jail.config_at(&conf);

        // (2) and (3). Planted first, so a directive naming the same file overwrites it and the
        // guard below drops the corresponding assertion.
        let planted_skips = SKIPPED
            .iter()
            .filter(|name| write_file(&conf, name, SKIPPED_BODY))
            .count();
        let order_planted = write_file(&conf, ORDER_FIRST, "[order]\nwinner = \"first\"\n")
            && write_file(&conf, ORDER_LAST, "[order]\nwinner = \"last\"\n");

        let mut planted_disturbed = false;
        for directive in directives(data) {
            let Directive::File { name, content } = directive else {
                continue;
            };
            // A fragment that names a planted file, or that defines either planted key itself,
            // would be deciding the assertion instead of the loader.
            if SKIPPED.contains(&name)
                || name == ORDER_FIRST
                || name == ORDER_LAST
                || content.contains(SENTINEL_KEY)
                || content.contains("order")
            {
                planted_disturbed = true;
            }
            write_file(&conf, name, &content);
        }

        // (1) Totality. Corrupted TOML is a legitimate typed failure.
        let Ok(loaded) = jail.load_watched::<Value>() else {
            return Ok(());
        };

        if !planted_disturbed {
            // (2) Nothing the expansion refuses may contribute.
            if planted_skips > 0 {
                assert!(
                    !contains_key(&loaded.value, SENTINEL_KEY),
                    "an entry that is dot-prefixed or not `*.toml` reached the configuration"
                );
            }

            // (3) Later name wins.
            if order_planted {
                assert_eq!(
                    lookup(&loaded.value, ORDER_KEY).and_then(Value::as_str),
                    Some("last"),
                    "fragments merged in the wrong order: `{ORDER_LAST}` must outrank \
                     `{ORDER_FIRST}`"
                );
            }
        }

        // (4) Re-reading an untouched directory is not a change.
        let again = jail
            .load_watched::<Value>()
            .expect("a load that just succeeded must succeed again unchanged");
        assert!(
            !again.sources.differs_from(&loaded.sources),
            "an unchanged configuration directory reported a change"
        );

        assert!(
            loaded
                .sources
                .watch_paths()
                .iter()
                .any(|path| path == &conf),
            "the configuration directory is not in the watch set"
        );

        Ok(())
    });
}
