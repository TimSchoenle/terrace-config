//! Replays the committed corpus through the oracles, without libFuzzer.
//!
//! Two jobs. The first is regression: every seed under `seeds/` — and every input a campaign
//! promoted into `corpus/` — is run through the matching oracle on a plain `cargo test`, so a
//! reproducer keeps being checked long after whoever found it moved on.
//!
//! The second is validating the oracles themselves. An oracle that models the loader wrongly
//! reports crashes that are not bugs, and the way to find that out is to run it over inputs
//! designed to hit its edges — two spellings of one key, a name that is only separators, a
//! collision between every pair of layers. [`generated`] does that from a fixed seed, so a
//! failure is reproducible from the test name alone rather than from a `corpus/` blob.

use std::path::{Path, PathBuf};

use terrace_config_fuzz::oracle;

/// The oracle under test, chosen by the seed directory's name.
type Oracle = fn(&str);

fn seed_dir(target: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("seeds")
        .join(target)
}

fn corpus_dir(target: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join(target)
}

/// Run every file in `dir` through `oracle`, naming the file if it panics.
///
/// A directory that does not exist is not a failure: `corpus/` is where a campaign writes, and a
/// fresh clone has only the `.gitkeep`.
fn replay(dir: &Path, oracle: Oracle) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };

    let mut replayed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.file_name().is_some_and(|n| n == ".gitkeep") {
            continue;
        }
        // Non-UTF-8 is skipped rather than failed: the targets take `&str`, so libFuzzer only
        // ever hands them valid UTF-8, and a corpus file that is not is not their input.
        let Ok(data) = std::fs::read_to_string(&path) else {
            continue;
        };
        // `catch_unwind` is deliberately *not* used. A panic is the finding, and the test
        // harness already names the test and prints the payload; wrapping it would only make
        // the report worse.
        oracle(&data);
        replayed += 1;
    }
    replayed
}

fn replay_target(target: &str, oracle: Oracle) {
    let seeds = replay(&seed_dir(target), oracle);
    assert!(
        seeds > 0,
        "no seeds found for `{target}` — the corpus is what makes this test mean anything"
    );
    replay(&corpus_dir(target), oracle);
}

#[test]
fn env_load_seeds() {
    replay_target("env_load", oracle::env_load::check);
}

#[test]
fn explain_seeds() {
    replay_target("explain", oracle::explain::check);
}

#[test]
fn secrets_dir_seeds() {
    replay_target("secrets_dir", oracle::secrets_dir::check);
}

#[test]
fn toml_layers_seeds() {
    replay_target("toml_layers", oracle::toml_layers::check);
}

#[test]
fn schema_seeds() {
    replay_target("schema", oracle::schema::check);
}

#[test]
fn kube_seeds() {
    replay_target("kube", oracle::kube::check);
}

/// A deterministic input generator, standing in for a mutation engine.
///
/// Not a substitute for a real campaign — it has no coverage feedback, so it explores by
/// combination rather than by discovery. What it is good at is the thing a campaign is slow at:
/// hitting every *pairing* of the interesting tokens quickly, which is where an oracle that
/// models the loader wrongly gives itself away.
mod generated {
    use super::{Oracle, oracle};

    /// xorshift64*, so the sequence is fixed across platforms and runs. A failure reproduces
    /// from the test name and the iteration index printed with it.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn pick<'a, T>(&mut self, options: &'a [T]) -> &'a T {
            let index = usize::try_from(self.next() % options.len() as u64).expect("fits");
            &options[index]
        }
    }

    /// Names chosen to sit on a rule boundary: the nesting separator doubled and tripled, a
    /// dotted name, a dot-prefixed name, the reserved keys, and two spellings of one key.
    const NAMES: &[&str] = &[
        "auth__jwt_secret",
        "AUTH__JWT_SECRET",
        "a",
        "a__b",
        "a____b",
        "a___b",
        "__",
        "_",
        "auth.jwt_secret",
        "..data",
        ".hidden",
        "profile",
        "config",
        "secrets_dir",
        "sentinel_hidden",
        "ünïcode",
        "x.toml",
        "00-first.toml",
        "10-base.toml",
        "zz-last.toml",
    ];

    /// Values chosen to sit on the trimming rule and on figment's value parsing.
    const VALUES: &[&str] = &[
        "v",
        "",
        "12345678",
        "trailing\\n",
        "\\r\\n",
        "\\n\\n\\n",
        " padded ",
        "nan",
        "true",
        "-1",
        "[database]\\nurl = \"x\"\\n",
        "key = \"value\"\\n",
        "[order]\\nwinner = \"forged\"\\n",
        "[sentinel]\\nleaked = true\\n",
        "not toml at all [",
    ];

    const KINDS: &[&str] = &["f", "e", "p", "t", "q"];

    /// Build one input of up to five directives.
    fn generate(rng: &mut Rng) -> String {
        let lines = 1 + rng.next() % 5;
        (0..lines)
            .map(|_| {
                format!(
                    "{}:{}={}",
                    rng.pick(KINDS),
                    rng.pick(NAMES),
                    rng.pick(VALUES)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The default budget per target. Large enough to pair most tokens, small enough that the
    /// suite stays a few seconds — every iteration builds a real temp directory.
    const DEFAULT_ITERATIONS: usize = 600;

    /// The budget, overridable so a longer hunt does not need a recompile.
    ///
    /// Read before the oracle runs, because the jail each one enters clears the environment.
    fn iterations() -> usize {
        std::env::var("TERRACE_FUZZ_ITERATIONS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_ITERATIONS)
    }

    /// Run `oracle` over the generated inputs, naming the one that fails.
    ///
    /// `catch_unwind` rather than a panic hook, which was the first attempt and was wrong: a hook
    /// is process-global, these tests run in parallel, and the input it printed belonged to
    /// whichever sweep installed the hook last. `catch_unwind` attributes the failure to the
    /// iteration that actually produced it.
    ///
    /// The corpus replay above deliberately does *not* do this — there the file name is the
    /// context, and the harness already prints it.
    fn sweep(seed: u64, oracle: Oracle) {
        sweep_with(seed, oracle, generate);
    }

    /// As [`sweep`], for a target whose input grammar is its own.
    fn sweep_with(seed: u64, oracle: Oracle, generate: fn(&mut Rng) -> String) {
        let mut rng = Rng(seed);
        for iteration in 0..iterations() {
            let input = generate(&mut rng);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| oracle(&input)));
            assert!(
                outcome.is_ok(),
                "seed {seed:#x}, iteration {iteration} failed on input:\n{input}"
            );
        }
    }

    #[test]
    fn env_load_sweep() {
        sweep(0x5EED_0001, oracle::env_load::check);
    }

    #[test]
    fn explain_sweep() {
        sweep(0x5EED_0005, oracle::explain::check);
    }

    #[test]
    fn secrets_dir_sweep() {
        sweep(0x5EED_0002, oracle::secrets_dir::check);
    }

    #[test]
    fn toml_layers_sweep() {
        sweep(0x5EED_0003, oracle::toml_layers::check);
    }

    /// Path segments chosen to sit on the rules that decide whether a key has an environment
    /// spelling at all: case that does not survive the fold, the separator embedded in a segment,
    /// a `.` inside one, and a name ending in the indirection suffix.
    const SEGMENTS: &[&str] = &[
        "a",
        "b",
        "auth",
        "jwt_secret",
        "AUTH",
        "distDir",
        "MiXeD",
        "a__b",
        "a_b",
        "a.b",
        "file",
        "filename",
        "_",
        "__",
        "profile",
        "config",
        "secrets_dir",
        "ünïcode",
        "x_FILE",
        "0",
        // Characters no environment variable name can carry.
        "a\0b",
        "a=b",
        // Characters that end a Markdown cell. A deterministic sweep can only combine the
        // alphabet it is given, and leaving these out of it is why a libFuzzer campaign
        // found the unescaped `Path` column first.
        "a|b",
        "a\\b",
        // Whitespace, which figment's environment layer trims off the key it produces --
        // so a path carrying it is reachable by a file and not by a variable.
        "by ",
        " by",
        " ",
        "",
        // A leading or trailing dot, which makes a segment whose parent is the empty
        // prefix -- the branch of `subset` that means "the whole schema".
        ".ab",
        "ab.",
    ];

    /// Separators and suffixes that each break a different assumption: one that is a prefix of
    /// another, one containing a letter, one that is a single character, one containing a `.`.
    const SEPARATORS: &[&str] = &[
        "__", "_", "_X_", "-", ".", "___", "x", "|",
        // `=` and a NUL cannot be in a variable name, and `/` would make a secrets-directory
        // entry a path rather than a name: each must produce *no* spelling, not a bad one.
        "=", "/", "\0",
    ];
    const SUFFIXES: &[&str] = &["_FILE", "_PATH", "_", "FILE", "_file"];

    /// Types and value sets, including the ones whose rendering needs escaping: a generic with a
    /// comma in it, and a choice list whose separator is the character that ends a table cell.
    const TYPES: &[&str] = &[
        "String",
        "u16",
        "Vec<String>",
        "BTreeMap<String, u8>",
        "std::path::PathBuf",
        "Log|Level",
        "",
    ];
    const VALUE_SETS: &[&str] = &["trace,debug,info", "on,off", "a", "a,,b", "x|y,z", ""];

    /// Prose chosen to break a Markdown table if it is not escaped.
    const PROSE: &[&str] = &[
        "plain",
        "",
        "pipes | in | prose",
        r"back\slash",
        r"escaped \| pipe",
        "em — dash",
        "trailing space ",
    ];

    /// Build one schema-oracle input.
    fn generate_schema(rng: &mut Rng) -> String {
        let mut lines = Vec::new();
        if rng.next().is_multiple_of(3) {
            lines.push(format!("s:{}", rng.pick(SEPARATORS)));
        }
        if rng.next().is_multiple_of(4) {
            lines.push(format!("x:{}", rng.pick(SUFFIXES)));
        }
        if rng.next().is_multiple_of(5) {
            lines.push(format!("r:TEST_{}", rng.pick(SEGMENTS).to_uppercase()));
        }

        let keys = 1 + rng.next() % 5;
        for _ in 0..keys {
            let depth = 1 + rng.next() % 3;
            let path = (0..depth)
                .map(|_| (*rng.pick(SEGMENTS)).to_owned())
                .collect::<Vec<_>>()
                .join("/");
            lines.push(format!("k:{path}={}", rng.pick(PROSE)));
            if rng.next().is_multiple_of(3) {
                lines.push(format!("t:{path}={}", rng.pick(TYPES)));
            }
            if rng.next().is_multiple_of(4) {
                lines.push(format!("v:{path}={}", rng.pick(VALUE_SETS)));
            }
            if rng.next().is_multiple_of(5) {
                lines.push(format!("A:{path}={}", rng.pick(SEGMENTS)));
            }
            if rng.next().is_multiple_of(3) {
                lines.push(format!("m:{path}={}", rng.pick(PROSE)));
            }
            if rng.next().is_multiple_of(4) {
                lines.push(format!("S:{path}"));
            }
        }
        lines.join(
            "
",
        )
    }

    #[test]
    fn schema_sweep() {
        sweep_with(0x5EED_0004, oracle::schema::check, generate_schema);
    }

    /// Loader prefixes chosen to sit on the label-value rule rather than on the loader's: one that
    /// is legal as a label value, one that ends in the separator every real prefix ends in, one
    /// longer than a label value may be, and the empty one the contract builder refuses outright.
    ///
    /// None of them reaches the stamp, and that is the property. A sweep that could not vary them
    /// could not notice one of them starting to.
    const PREFIXES: &[&str] = &[
        "TEST_",
        "PORTFOLIO_",
        "a",
        "A_SERVICE_PREFIX_CONSIDERABLY_LONGER_THAN_SIXTY_THREE_CHARACTERS_LONG_",
        ".",
        "",
    ];

    /// App names, for the same reason: prose, a slash, a plus, and one that is only whitespace.
    const APP_NAMES: &[&str] = &["portfolio", "not/a label+value", "  ", ""];

    /// Document keys chosen to sit on the `ConfigMap` data-key rule: the two names a directory
    /// already has, a path, a name a projected volume reserves, and one that is only a dot.
    const DOCUMENT_KEYS: &[&str] = &[
        "config.toml",
        "00-base.toml",
        ".",
        "..",
        "..data",
        "conf/ig.toml",
        "config toml",
        "_",
        "",
    ];

    /// Syntax names, including the two shapes a format may not take — empty, and a media type.
    const FORMATS: &[&str] = &["toml", "yaml", "json", "hcl", "TOML", "text/toml", ""];

    /// References chosen to sit on the one rule the pairing rests on: pinned, pinned with a
    /// registry port, tag-only, a digest of the wrong length, upper case, the wrong algorithm, and
    /// one carrying the separator the annotation joins on.
    const REFERENCES: &[&str] = &[
        "ghcr.io/you/portfolio@sha256:\
         48e259cb0e5f4b3a6d1c8f97a2b4e6d0c3a5f7192b4d6e8a0c2e4f6a8b0d2e4f",
        "ghcr.io:5000/you/sidecar:v2.5.0@sha256:\
         9f1c37ad5e0b2c4d6e8f0a1b3c5d7e9f0a2b4c6d8e0f1a3b5c7d9e1f3a5b7c9d",
        "portfolio@sha256:\
         c0ffee11223344556677889900aabbccddeeff00112233445566778899aabbcd",
        "ghcr.io/you/portfolio:v2.5.0",
        "ghcr.io/you/portfolio@sha256:deadbeef",
        "ghcr.io/you/portfolio@sha512:0123456789abcdef",
        "portfolio@sha256:\
         48E259CB0E5F4B3A6D1C8F97A2B4E6D0C3A5F7192B4D6E8A0C2E4F6A8B0D2E4F",
        "a@b@sha256:48e259cb0e5f4b3a6d1c8f97a2b4e6d0c3a5f7192b4d6e8a0c2e4f6a8b0d2e4f",
        "one@sha256:48e259cb0e5f4b3a6d1c8f97a2b4e6d0c3a5f7192b4d6e8a0c2e4f6a8b0d2e4f,two",
        " portfolio@sha256:48e259cb0e5f4b3a6d1c8f97a2b4e6d0c3a5f7192b4d6e8a0c2e4f6a8b0d2e4f",
        "",
    ];

    /// Build one kube-oracle input.
    fn generate_kube(rng: &mut Rng) -> String {
        let mut lines = vec![
            format!("p:{}", rng.pick(PREFIXES)),
            format!("n:{}", rng.pick(APP_NAMES)),
            format!("d:{}", rng.pick(DOCUMENT_KEYS)),
            format!("f:{}", rng.pick(FORMATS)),
        ];
        let keys = 1 + rng.next() % 4;
        for _ in 0..keys {
            let depth = 1 + rng.next() % 3;
            let path = (0..depth)
                .map(|_| (*rng.pick(SEGMENTS)).to_owned())
                .collect::<Vec<_>>()
                .join("/");
            lines.push(format!("k:{path}"));
        }
        let images = 1 + rng.next() % 3;
        for _ in 0..images {
            lines.push(format!("i:{}", rng.pick(REFERENCES)));
        }
        lines.join("\n")
    }

    #[test]
    fn kube_sweep() {
        sweep_with(0x5EED_0006, oracle::kube::check, generate_kube);
    }
}
