//! Fuzzes the two file-backed layers against a directory the fuzzer builds: `SecretsDir` over
//! `$TEST_SECRETS_DIR`, and `FileSuffixEnv` over `TEST_<KEY>_FILE` indirection.
//!
//! This is the oracle that matters. The secrets-directory provider is the one part of this crate
//! with no equivalent anywhere on crates.io, its rules are a list of things that failed in
//! production, and unlike the environment layer its input genuinely is attacker-shaped — a
//! mounted `Secret` is written by whatever wrote the `Secret`.
//!
//! # Oracle
//! 1. **Totality.** `Ok` or [`terrace_config::Error`], never a panic.
//! 2. **Exact success or failure.** This module computes, independently of the loader, whether
//!    the directory it built should load, and asserts the answer. The claim that carries weight
//!    is the failing half: **a key supplied by two mechanisms must never be silently accepted.**
//!    That is the crate's second reason to exist, and a regression to precedence semantics would
//!    keep every existing test green.
//! 3. **The trimming contract.** A value equals its file's contents minus trailing `\r` and
//!    `\n`, and nothing else. Restated here rather than called through, so the comparison is
//!    against an independent statement of the rule.
//! 4. **The skip contract, and its ordering.** A planted `.sentinel_hidden` must neither reach
//!    the config nor fail the load. Both halves matter: the dot-prefix skip has to happen
//!    *before* the dotted-name rejection, or a `ConfigMap`'s own `..data` entry would fail every
//!    boot in the cluster.
//! 5. **`LastWins` never fails where `Reject` succeeds.** The relaxed policy takes a different
//!    path through the same collection, and it must not be the stricter one.
//!
//! # Input shape
//! One directive per line; `\n`, `\r` and `\\` are decoded in file bodies so the trailing-
//! terminator rule is reachable from a line-oriented grammar.
//!
//! ```text
//! f:<name>=<content>     a file in the secrets directory
//! p:<SUFFIX>=<content>   a TEST_<SUFFIX>_FILE indirection at a path this module names
//! e:<SUFFIX>=<value>     the plain environment variable TEST_<SUFFIX>
//! ```
//!
//! Indirection **paths** are never taken from the input — only the contents of the file this
//! module itself creates. A fuzzer-chosen path would have it reading arbitrary files on the
//! host, which is a harness bug waiting to be reported as a crate bug.

use std::collections::{BTreeMap, BTreeSet};

use figment::value::Value;
use terrace_config::testing::Harness;
use terrace_config::{ShadowPolicy, Terrace};

use crate::support::{
    Directive, PREFIX, contains_value, directives, expected_value, is_safe_env, lookup, write_file,
};

/// Read straight from the environment by a hypothetical consumer, so no file may supply it.
/// `TEST_CONFIG` and `TEST_SECRETS_DIR` are reserved by the builder without being named.
const RESERVED: &str = "TEST_PROFILE";

/// Reserved in full, as this module must reason about them. `TEST_SECRETS_DIR` is also this
/// module's own control variable.
const ALL_RESERVED: &[&str] = &[RESERVED, "TEST_CONFIG", "TEST_SECRETS_DIR"];

/// Whether an environment name is one of [`ALL_RESERVED`], **ignoring case**.
///
/// Case-insensitively, and that is not fussiness. Windows environment names are
/// case-insensitive, so setting `TEST_secrets_dir` overwrites the `TEST_SECRETS_DIR` this module
/// pointed at the directory it built — the load then fails on a directory that does not exist,
/// correctly, and the model has no way to know. A generated input found exactly that.
///
/// The loader's own [`terrace_config::Dialect`] compares reserved names exactly, which is right
/// on Linux, where the two really are different variables. Matching loosely here costs a little
/// coverage on Linux and buys a harness that means the same thing on both.
fn is_reserved(name: &str) -> bool {
    ALL_RESERVED
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(name))
}

/// Planted every iteration. Dot-prefixed, so the loader must skip it — and must skip it *before*
/// noticing that the name also contains a `.`, which on its own is a hard error.
const SENTINEL: &str = ".sentinel_hidden";

/// Planted contents, and the thing actually asserted about.
///
/// Not the *key*, which was the first attempt and was almost useless: every dot-prefixed name
/// also contains a dot, so a loader that stopped skipping them would reject the whole directory
/// rather than leak it — a failure the exact-outcome oracle already catches. The reachable bug
/// is the plausible "fix" of stripping the leading dot instead of skipping the entry, and that
/// arrives under an ordinary key. Only the contents give it away.
const SENTINEL_BODY: &str = "terrace-fuzz-sentinel-must-not-be-read";

fn layers() -> Terrace {
    Terrace::new(PREFIX).reserve(RESERVED)
}

/// The sandbox each iteration runs in: an empty environment, restored afterwards, around the
/// loader under test. `std::env::set_var` is `unsafe` in edition 2024 and both crates forbid
/// unsafe code, so a jail is not a convenience here — it is the only way in.
fn harness() -> Harness {
    Harness::over(layers())
}

/// The figment key path an environment suffix or file name denotes.
///
/// A restatement of the rule, not a call into it: `split(sep).join(".")` is what the loader
/// does, and `replace` is the same function written the other way round.
fn key_path(spelling: &str) -> String {
    spelling.to_ascii_lowercase().replace("__", ".")
}

/// What the loader should make of one file name, decided without asking it.
enum Verdict {
    /// Skipped silently: dot-prefixed.
    Ignored,
    /// A hard error: a `.` that is not the nesting separator, or a reserved key.
    Fatal,
    /// Contributes this key path.
    Key(String),
}

fn verdict(name: &str) -> Verdict {
    // Order matters, and is the property: dot-prefix first, dotted-name second.
    if name.starts_with('.') {
        return Verdict::Ignored;
    }
    if name.contains('.') {
        return Verdict::Fatal;
    }
    if is_reserved(&format!("{PREFIX}{name}")) {
        return Verdict::Fatal;
    }
    Verdict::Key(key_path(name))
}

/// Whether any key in `keys` is a strict dot-prefix of another.
///
/// `a` and `a__b` disagree about whether `a` is a leaf, and the loader resolves that by letting
/// the later one replace the earlier. That is documented behaviour, but it makes a per-key value
/// assertion ambiguous, so none is made.
fn has_nesting_conflict(keys: &BTreeSet<String>) -> bool {
    keys.iter().any(|key| {
        let prefix = format!("{key}.");
        keys.iter()
            .any(|other| other != key && other.starts_with(&prefix))
    })
}

/// What the input was understood to build.
#[derive(Default)]
struct Model {
    /// Secrets-directory files, by key path.
    files: BTreeMap<String, String>,
    /// Indirection targets, by key path.
    indirect: BTreeMap<String, String>,
    /// Plain environment keys that participate in shadowing.
    env: BTreeSet<String>,
    /// A name that cannot be a key at all: the load must fail.
    fatal: bool,
    /// Two spellings normalised to one key, so the winner is directory read order.
    ambiguous: bool,
    /// A directive named the sentinel, so its assertion no longer belongs to the loader.
    sentinel_shadowed: bool,
}

impl Model {
    /// Whether any key arrives through more than one mechanism.
    fn collides(&self) -> bool {
        let files: BTreeSet<&String> = self.files.keys().collect();
        let indirect: BTreeSet<&String> = self.indirect.keys().collect();
        let env: BTreeSet<&String> = self.env.iter().collect();
        files.intersection(&indirect).next().is_some()
            || files.intersection(&env).next().is_some()
            || indirect.intersection(&env).next().is_some()
    }

    /// Every key path the model expects to exist, from any layer.
    fn all_keys(&self) -> BTreeSet<String> {
        self.files
            .keys()
            .chain(self.indirect.keys())
            .chain(self.env.iter())
            .cloned()
            .collect()
    }
}

/// Run the oracle. Panics when the loader breaks one of the rules above.
///
/// # Panics
/// That is the contract: a panic is the finding.
#[expect(
    clippy::too_many_lines,
    reason = "one pass that builds the directory and the model together; splitting them would               put the two halves of every rule in different functions, which is the one thing               an oracle must not do"
)]
pub fn check(data: &str) {
    // Ignored: a jail-setup failure says nothing about the code under test.
    let _ = harness().try_run(|jail| {
        let root = jail.sandbox().root().to_path_buf();
        let Ok(secrets) = jail.secrets_dir() else {
            return Ok(());
        };

        // (4) The planted skip. Written before the fuzzer's files so a directive naming the same
        // thing overwrites it, which the model then notices.
        let sentinel_planted = write_file(&secrets, SENTINEL, SENTINEL_BODY);

        let mut model = Model::default();
        let mut indirect_seq = 0_usize;

        for directive in directives(data) {
            match directive {
                Directive::File { name, content } => {
                    if !write_file(&secrets, name, &content) {
                        continue;
                    }
                    // A directive that writes the sentinel's own name, or its contents under
                    // some other name, is deciding the assertion instead of the loader.
                    if name.eq_ignore_ascii_case(SENTINEL) || content == SENTINEL_BODY {
                        model.sentinel_shadowed = true;
                    }
                    match verdict(name) {
                        Verdict::Ignored => {}
                        Verdict::Fatal => model.fatal = true,
                        Verdict::Key(key) => {
                            // Two names normalising to one key (`AUTH` and `auth`) leave the
                            // winner to directory read order, which is not a property.
                            if model.files.insert(key, content).is_some() {
                                model.ambiguous = true;
                            }
                        }
                    }
                }
                Directive::Indirect { suffix, content } => {
                    if !is_safe_env(suffix, "") {
                        continue;
                    }
                    // The path is this module's, never the input's. See the module doc.
                    indirect_seq += 1;
                    let name = format!("indirect-{indirect_seq}");
                    if !write_file(&root, &name, &content) {
                        continue;
                    }
                    let variable = format!("{PREFIX}{suffix}_FILE");
                    jail.env(&variable, root.join(&name).display());

                    if is_reserved(&format!("{PREFIX}{suffix}")) {
                        model.fatal = true;
                        continue;
                    }
                    if model.indirect.insert(key_path(suffix), content).is_some() {
                        model.ambiguous = true;
                    }
                }
                Directive::Env { suffix, value } => {
                    if !is_safe_env(suffix, value) {
                        continue;
                    }
                    let name = format!("{PREFIX}{suffix}");
                    // A `_FILE` name here would be an indirection with a fuzzer-chosen path.
                    if name.ends_with("_FILE") {
                        continue;
                    }
                    // Skipped rather than modelled: on Windows this *is* the control variable
                    // this module set, and overwriting it points the loader at a directory that
                    // does not exist. See `is_reserved`.
                    if is_reserved(&name) {
                        continue;
                    }
                    jail.env(&name, value);
                    model.env.insert(key_path(suffix));
                }
            }
        }

        let refused = model.fatal || model.collides();

        // (1) and (2). Totality first, then the verdict the model computed for itself.
        let loaded = match jail.load_watched::<Value>() {
            Err(_) => {
                assert!(
                    refused,
                    "the load failed but nothing in the directory justifies it"
                );
                return Ok(());
            }
            Ok(loaded) => {
                assert!(
                    !refused,
                    "the load succeeded with a key supplied twice, or a name that cannot be a key"
                );
                loaded
            }
        };

        // (4) The skip contract. Reaching here at all is half of it — a dot-prefixed entry that
        // stopped being skipped would be rejected for its dot and caught by (2) above. This is
        // the other half: it was not read under a rewritten name either.
        if sentinel_planted && !model.sentinel_shadowed {
            assert!(
                !contains_value(&loaded.value, SENTINEL_BODY),
                "a dot-prefixed entry was read into the configuration under a rewritten key"
            );
        }

        // (3) The trimming contract, where the merge is unambiguous. `collides` returned above,
        // so the two file maps are disjoint here.
        if !model.ambiguous && !has_nesting_conflict(&model.all_keys()) {
            for (key, content) in model.files.iter().chain(&model.indirect) {
                let found = lookup(&loaded.value, key)
                    .unwrap_or_else(|| panic!("`{key}` was supplied by a file but is not present"));
                assert_eq!(
                    found.as_str(),
                    Some(expected_value(content)),
                    "`{key}` lost or gained bytes that are not trailing line terminators"
                );
            }
        }

        // (5) The relaxed policy must not be the stricter one.
        assert!(
            jail.terrace()
                .shadow_policy(ShadowPolicy::LastWins)
                .load::<Value>()
                .is_ok(),
            "LastWins rejected a directory that Reject accepted"
        );

        Ok(())
    });
}
