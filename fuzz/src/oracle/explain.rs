//! Fuzzes [`Terrace::explain`] against a deployment the fuzzer builds out of all four layers.
//!
//! The report is the one part of this crate whose output is *meant* to be printed into a log
//! that is shipped, indexed and retained, which makes "no configuration value ever reaches it"
//! the single claim worth attacking with arbitrary input. Every other rule here exists because a
//! diagnostic that points at the wrong thing is worse than none.
//!
//! # Oracle
//! 1. **Totality.** `Ok` or [`terrace_config::Error`], never a panic — over arbitrary file names,
//!    arbitrary TOML, and arbitrary variable spellings.
//! 2. **Redaction.** A sentinel value is planted in all four layers every iteration and must
//!    appear in neither the [`Display`] nor the [`Debug`] rendering. The claim is about the
//!    *value*: a fuzzer-chosen file name or variable name legitimately appears in the report, so
//!    the assertion is suspended for the iteration when one of those carries the sentinel too.
//! 3. **Nothing is invented.** Every layer the report names must exist: a file it points at is on
//!    disk, a variable it names is set. A report that sends an operator to a path nobody wrote is
//!    the failure mode that makes a diagnostic worse than silence.
//! 4. **The invariant behind the type.** Every origin has one effective layer and
//!    `sources() == shadowed() ++ [effective()]`, so `is_contested` cannot disagree with what
//!    `shadowed` holds.
//! 5. **A refused boot is an explained boot.** If [`ShadowPolicy::Reject`] refuses to assemble
//!    while the report itself can be produced, the refusal was about a doubly-supplied key — so
//!    the report has to name at least one contested key. That is the whole of the feature's
//!    reason to exist: the loader's error names one pair, and the report names every one of them.
//! 6. **The report never fails where a load succeeds.** It assembles under
//!    [`ShadowPolicy::LastWins`] on purpose, so it must be at least as permissive as any load.
//!
//! # Input shape
//! One directive per line; `\n`, `\r` and `\\` are decoded in file bodies.
//!
//! ```text
//! f:<name>=<content>     a file — `*.toml` goes in the config directory, anything else in the
//!                        secrets directory, which is the split the loader itself makes
//! t:<name>=<content>     the same directive; both spellings exist so one corpus feeds every target
//! p:<SUFFIX>=<content>   a TEST_<SUFFIX>_FILE indirection at a path this module names
//! e:<SUFFIX>=<value>     the plain environment variable TEST_<SUFFIX>
//! ```
//!
//! Indirection **paths** are never taken from the input, for the reason given in
//! [`secrets_dir`](super::secrets_dir): a fuzzer-chosen path would have the target reading
//! arbitrary files on the host machine.

use figment::value::Value;
use terrace_config::explain::{Explanation, Layer};
use terrace_config::testing::Harness;
use terrace_config::{ShadowPolicy, Terrace};

use crate::support::{Directive, PREFIX, directives, is_safe_env, write_file};

/// Read straight from the environment by a hypothetical consumer, as in every other target.
const RESERVED: &str = "TEST_PROFILE";

/// The value planted in all four layers, and the only thing rule (2) asserts about.
///
/// A *value*, never a name. The report prints paths and variable spellings by design — that is
/// what makes it usable — so a sentinel that could be either would be asserting the opposite of
/// the contract.
const SENTINEL: &str = "terrace-fuzz-value-must-not-be-printed";

fn layers() -> Terrace {
    Terrace::new(PREFIX).reserve(RESERVED)
}

/// The sandbox each iteration runs in: an empty environment, restored afterwards, around the
/// loader under test. `std::env::set_var` is `unsafe` in edition 2024 and both crates forbid
/// unsafe code, so a jail is not a convenience here — it is the only way in.
fn harness() -> Harness {
    Harness::over(layers())
}

/// Whether a fuzzer-supplied name or variable suffix would put the sentinel into the report
/// legitimately, as part of a path or a variable spelling.
///
/// Case-insensitively, because Windows environment names are.
fn mentions_sentinel(name: &str) -> bool {
    name.to_ascii_lowercase().contains(SENTINEL)
}

/// Whether the report may name this layer at all — rule (3), one layer at a time.
fn exists(layer: &Layer) -> bool {
    match layer {
        Layer::Toml(path) | Layer::SecretsFile(path) => path.is_file(),
        Layer::Env(var) => std::env::var_os(var).is_some(),
        // Both halves: the variable is what an operator would unset, and the path is what they
        // would `cat`. A report naming one without the other sends them to the wrong place.
        Layer::Indirection { var, path } => std::env::var_os(var).is_some() && path.is_file(),
    }
}

/// Rules (3) and (4), over the whole report.
fn check_origins(explanation: &Explanation) {
    for origin in explanation.origins() {
        let sources: Vec<&Layer> = origin.sources().collect();
        let expected: Vec<&Layer> = origin
            .shadowed()
            .iter()
            .chain(std::iter::once(origin.effective()))
            .collect();
        assert_eq!(
            sources,
            expected,
            "`{}` reports a source list that is not its shadowed layers plus its effective one",
            origin.key()
        );
        assert_eq!(
            origin.is_contested(),
            !origin.shadowed().is_empty(),
            "`{}` disagrees with itself about being contested",
            origin.key()
        );
        for layer in sources {
            assert!(
                exists(layer),
                "`{}` is attributed to {layer}, which is not there",
                origin.key()
            );
        }
    }
}

/// Run the oracle. Panics when the report breaks one of the rules above.
///
/// # Panics
/// That is the contract: a panic is the finding.
pub fn check(data: &str) {
    // Ignored: a jail-setup failure says nothing about the code under test.
    let _ = harness().try_run(|jail| {
        let root = jail.sandbox().root().to_path_buf();

        // (2) The plant, in all four layers, and the two mounts arranged in the same breath —
        // both through the jail, so the variable naming each is the one the loader under test
        // actually reads rather than a spelling restated here. Written before the fuzzer's
        // directives, so one naming the same file overwrites it: that only weakens the
        // assertion, never breaks it.
        let sentinel_toml = format!("[sentinel]\ntoml = \"{SENTINEL}\"\n");
        let (Ok(fragment), Ok(secrets), Ok(_)) = (
            jail.fragment("00-sentinel.toml", &sentinel_toml),
            jail.secret("sentinel__dir", SENTINEL),
            jail.indirection("sentinel.indirect", SENTINEL),
        ) else {
            return Ok(());
        };
        jail.env_key("sentinel.env", SENTINEL);

        // The configuration *directory*, which is what `fragment` mounted and what the fuzzer's
        // own `*.toml` directives go into.
        let Some(config) = fragment.parent().map(std::path::Path::to_path_buf) else {
            return Ok(());
        };

        // A fuzzer-supplied *name* carrying the sentinel would put it in the report as a path or
        // a variable, which is correct behaviour and would fail an assertion about values.
        let mut sentinel_shadowed = false;
        let mut indirect_seq = 0_usize;

        for directive in directives(data) {
            match directive {
                Directive::File { name, content } => {
                    sentinel_shadowed |= mentions_sentinel(name);
                    // The split the loader makes: the config directory takes `*.toml` and
                    // ignores everything else, the secrets directory takes every other name.
                    let dir = if std::path::Path::new(name)
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
                    {
                        &config
                    } else {
                        &secrets
                    };
                    write_file(dir, name, &content);
                }
                Directive::Indirect { suffix, content } => {
                    if !is_safe_env(suffix, "") {
                        continue;
                    }
                    sentinel_shadowed |= mentions_sentinel(suffix);
                    // The path is this module's, never the input's. See the module doc.
                    indirect_seq += 1;
                    let name = format!("indirect-{indirect_seq}");
                    if !write_file(&root, &name, &content) {
                        continue;
                    }
                    jail.env(format!("{PREFIX}{suffix}_FILE"), root.join(&name).display());
                }
                Directive::Env { suffix, value } => {
                    if !is_safe_env(suffix, value) {
                        continue;
                    }
                    sentinel_shadowed |= mentions_sentinel(suffix);
                    let name = format!("{PREFIX}{suffix}");
                    // A `_FILE` name here would be an indirection with a fuzzer-chosen path, and
                    // the two control variables decide what this module built.
                    if name.ends_with("_FILE")
                        || name.eq_ignore_ascii_case("TEST_CONFIG")
                        || name.eq_ignore_ascii_case("TEST_SECRETS_DIR")
                    {
                        continue;
                    }
                    jail.env(&name, value);
                }
            }
        }

        // (1) Totality. A readable deployment that cannot be explained is the finding; an
        // unreadable one is an answer, and the loader raises the same error for it.
        let Ok(explanation) = layers().explain() else {
            // (6), in its contrapositive: a load that succeeds cannot be one the report refuses.
            assert!(
                layers()
                    .shadow_policy(ShadowPolicy::LastWins)
                    .load::<Value>()
                    .is_err(),
                "the report refused a deployment that loaded"
            );
            return Ok(());
        };

        check_origins(&explanation);

        // (2). Both renderings, because they are two ways for one field to escape and the type's
        // whole claim is that there is no such field.
        if !sentinel_shadowed {
            for rendered in [format!("{explanation}"), format!("{explanation:?}")] {
                assert!(
                    !rendered.contains(SENTINEL),
                    "a configuration value reached the report:\n{rendered}"
                );
            }
        }

        // (5). `figment()` rather than `load()`: it stops after assembly, so its failure is the
        // shadow check refusing and never a `serde` extraction that says nothing about layering.
        if layers().figment().is_err() {
            assert!(
                explanation.contested().next().is_some(),
                "the default policy refused to assemble, and the report names no contested key"
            );
        }

        Ok(())
    });
}
