//! Replays the committed corpus through the oracles, without libFuzzer.
//!
//! Two jobs, and the second is the one that is easy to skip.
//!
//! The first is **regression**: every seed under `seeds/` — and every input a campaign promoted
//! into `corpus/` — runs through the matching oracle on a plain `cargo test`, so a reproducer keeps
//! being checked long after whoever found it moved on. Each seed here sits on a named rule: one per
//! refusal, one per way a rendering can be handed text it cannot write raw.
//!
//! The second is **validating the oracles themselves**. An oracle that models a rule wrongly
//! reports findings that are not defects, and the way to find that out is to run it over inputs
//! designed to hit its edges. [`generated`] does that from a fixed seed, so a failure reproduces
//! from the test name and the printed iteration index rather than from a `corpus/` blob somebody
//! has to find.

use std::path::{Path, PathBuf};

use terrace_contract_fuzz::oracle;

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
        if !path.is_file() || path.file_name().is_some_and(|name| name == ".gitkeep") {
            continue;
        }
        // Non-UTF-8 is skipped rather than failed: the targets take `&str`, so libFuzzer only ever
        // hands them valid UTF-8, and a corpus file that is not is not their input.
        let Ok(data) = std::fs::read_to_string(&path) else {
            continue;
        };
        // The seed's name is printed before it runs rather than caught after: a panic is the
        // finding, and the harness already prints the payload. Wrapping it in `catch_unwind` would
        // only make the report worse.
        eprintln!("replaying {}", path.display());
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
fn document_seeds() {
    replay_target("document", oracle::document::check);
}

#[test]
fn render_seeds() {
    replay_target("render", oracle::render::check);
}

#[test]
fn conform_seeds() {
    replay_target("conform", oracle::conform::check);
}

/// A deterministic input generator, standing in for a mutation engine.
///
/// Not a substitute for a real campaign — it has no coverage feedback, so it explores by
/// combination rather than by discovery. What it is good at is the thing a campaign is slow at:
/// hitting every *pairing* of the interesting tokens quickly, which is where a renderer that
/// escapes one hostile character and not its neighbour gives itself away.
mod generated {
    use super::{Oracle, oracle};

    /// xorshift64*, so the sequence is fixed across platforms and runs.
    ///
    /// A failure reproduces from the test name and the iteration index printed with it, which is
    /// the whole reason this is not `rand`: a generator seeded from the clock finds a defect once
    /// and can never be asked about it again.
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

    const CASES: &[&str] = &["minimal", "full-surface", "unnameable-key", "nonsense"];

    /// Directives chosen to sit on a rule boundary rather than to be plausible.
    ///
    /// Each of these is one edge some rule has: the prefix emptied, a pattern that subsumes it, a
    /// name that is only separators, a type name from another language, a default in each of the
    /// shapes the TOML writer treats differently.
    const DIRECTIVES: &[&str] = &[
        "p=",
        "p=X_",
        "p=PORT",
        "n=_",
        "n=",
        "n=__",
        "i=_FILE",
        "i=",
        "L=figment",
        "L=spring-boot",
        "L=",
        "V=1",
        "V=2",
        "S=1",
        "S=2",
        "S=9",
        "r",
        "x=PORT",
        "x=PORTFOLIO_X",
        "x=RUST_LOG",
        "x=",
        "g=*",
        "g=PORT*",
        "g=PORTFOLIO_*",
        "g=KUBERNETES_*",
        "g=",
        "u=reject",
        "u=warn",
        "u=nonsense",
        "K=a",
        "K=a.b",
        "K=a|b",
        "K=",
        "K=a.b.c.d.e",
        "k=0:secret=true",
        "k=0:required=true",
        "k=0:reserved=true",
        "k=0:env=null",
        "k=0:env=WRONG",
        "k=0:unreachable=null",
        "k=0:unreachable=indirection",
        "k=0:unreachable=unnameable",
        "k=0:path=a|b",
        "k=0:path=",
        "k=0:path=a.b",
        "k=0:ty=java.time.Duration",
        "k=0:ty=null",
        "k=0:default=a|b",
        "k=0:default=null",
        "k=0:default_value=null",
        "k=0:default_value=0",
        "k=0:default_value=1.0",
        "k=0:default_value=\"\"",
        "k=0:default_value=[]",
        "k=0:default_value={}",
        "k=0:default_value=18446744073709551615",
        "k=0:constraint=null",
        "k=0:constraint={\"type\":\"integer\"}",
        "k=0:constraint={\"type\":\"array\"}",
        "k=0:constraint={\"type\":\"boolean\"}",
        "k=0:text_form=structured",
        "k=1:secret=true",
        "k=2:aliases=[\"x\",\"y\"]",
        "k=3:default_value={\"a\":[1,{\"b\":null}]}",
    ];

    /// Build one input of up to six directives.
    fn generate(rng: &mut Rng) -> String {
        let lines = 1 + rng.next() % 6;
        let mut input = String::from(*rng.pick(CASES));
        for _ in 0..lines {
            input.push('\n');
            input.push_str(rng.pick(DIRECTIVES));
        }
        input
    }

    /// The default budget per target. Large enough to pair most tokens, small enough that the
    /// suite stays a couple of seconds.
    const DEFAULT_ITERATIONS: usize = 2000;

    /// The budget, overridable so a longer hunt does not need a recompile.
    fn iterations() -> usize {
        std::env::var("TERRACE_FUZZ_ITERATIONS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_ITERATIONS)
    }

    /// Run `oracle` over the generated inputs, naming the one that fails.
    ///
    /// `catch_unwind` here and not in [`super::replay`], and the difference is the point: a seed
    /// has a file name that identifies it, and a generated input has only its index. Without this
    /// the panic names neither, and the input that found it is unreachable.
    fn sweep(name: &str, seed: u64, oracle: Oracle) {
        let mut rng = Rng(seed);
        for iteration in 0..iterations() {
            let input = generate(&mut rng);
            let outcome = std::panic::catch_unwind(|| oracle(&input));
            assert!(
                outcome.is_ok(),
                "`{name}` iteration {iteration} (seed {seed}) failed on:\n{input}"
            );
        }
    }

    /// Seeds chosen once and then left alone. Changing one throws away every input it reached.
    const DOCUMENT_SEED: u64 = 0x5EED_D0C0_1234_0001;
    const RENDER_SEED: u64 = 0x5EED_D0C0_1234_0002;
    const CONFORM_SEED: u64 = 0x5EED_D0C0_1234_0003;

    #[test]
    fn generated_documents() {
        // The document oracle takes raw JSON rather than directives, so it is fed the *rendered*
        // mutation — which is what a producer would actually hand a reader.
        let mut rng = Rng(DOCUMENT_SEED);
        for iteration in 0..iterations() {
            let directives = generate(&mut rng);
            let document = terrace_contract_fuzz::mutate::render(
                &terrace_contract_fuzz::mutate::document(&directives),
            );
            let outcome = std::panic::catch_unwind(|| oracle::document::check(&document));
            assert!(
                outcome.is_ok(),
                "`document` iteration {iteration} failed on:\n{directives}\n{document}"
            );
        }
    }

    #[test]
    fn generated_renderings() {
        sweep("render", RENDER_SEED, oracle::render::check);
    }

    #[test]
    fn generated_conformance() {
        sweep("conform", CONFORM_SEED, oracle::conform::check);
    }

    /// Truncations of a real document, which is the shape a reader is likeliest to meet in anger.
    ///
    /// A registry read that stops short, a file written by a build that was killed. Every prefix of
    /// a valid document has to be refused rather than half-read, and the reader has no business
    /// panicking on any of them.
    #[test]
    fn every_truncation_of_a_real_document_is_refused_quietly() {
        let full = terrace_contract_fuzz::mutate::render(&terrace_contract_fuzz::mutate::document(
            "full-surface",
        ));
        for end in 0..full.len() {
            if !full.is_char_boundary(end) {
                continue;
            }
            let truncated = &full[..end];
            let outcome = std::panic::catch_unwind(|| oracle::document::check(truncated));
            assert!(
                outcome.is_ok(),
                "truncating a real document to {end} bytes panicked the reader"
            );
        }
    }
}
