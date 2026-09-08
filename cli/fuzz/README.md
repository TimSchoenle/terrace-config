# terrace-contract fuzzing

Three targets, and the oracles live in `src/oracle/` rather than in the target binaries.

`cargo fuzz` needs a nightly sanitizer, and on Windows an `AddressSanitizer` runtime that ships
with Visual Studio rather than with rustup. **An oracle that only runs under that toolchain is an
oracle nobody checks** — so the bodies are in a library, `fuzz_targets/` is three shims, and the
whole committed corpus replays on a plain `cargo test`, anywhere:

```bash
cd cli/fuzz
cargo test                                    # every seed, plus a fixed-seed sweep
TERRACE_FUZZ_ITERATIONS=200000 cargo test     # a longer hunt, no recompile
cargo +nightly fuzz run render                # a real campaign, where the toolchain allows it
```

## The targets

| Target | Input | What it asserts |
|---|---|---|
| `document` | raw JSON | the envelope gate holds, and reading is a byte-stable fixed point — without which `stamp` would rewrite bytes nobody asked it to |
| `render` | mutation directives | every rendering keeps the promise it makes: the TOML file *parses*, the JSON Schema's `properties` is an object, every Markdown row has its header's column count, the Dockerfile block can be read back, and a placeholder's shape comes from `constraint` rather than from `ty` |
| `conform` | mutation directives | the rules are deterministic, tier 2 extends tier 1 rather than replacing it, every violation names a location, and the meta-schema and the rules agree where both have an opinion |

## Why the input is a directive language

Raw bytes fed to a contract reader produce JSON that fails to parse essentially every time, and an
oracle that only ever exercises the error path proves the reader rejects garbage — which was never
in doubt. The interesting inputs are documents that are *almost* right: a real one with its prefix
emptied, a secret given a default, an ignore pattern widened by one character.

So `src/mutate.rs` reads the input as directives applied to one of the stored conformance cases.
The fuzzer mutates directives; the directives mutate a document that was valid to begin with; and
what reaches the oracle is deep enough for a rule to have an opinion about it.

```
full-surface
k=0:secret=true
g=PORT*
```

`document` is the exception and takes raw JSON, because refusing cleanly is part of its job and the
shapes it must refuse — a truncated object, an array at the root — are exactly the ones a mutation
over a valid document never produces.

## Reproducibility

Nothing here reads a clock or an entropy source. The generator is xorshift64\* with a seed written
down in the source, so a failure reproduces from the test name and the iteration index printed with
it. `rand` would find a defect once and never be able to be asked about it again.

Changing a seed throws away every input it reached. Add directives instead.

## Seeds

`seeds/` is committed and is the regression suite: one input per refusal, one per way a rendering
can be handed text it cannot write raw. `corpus/` is where a campaign writes, is replayed if it has
anything in it, and is not committed.

When a campaign finds something, the fix is two commits' worth of work in one: the repair, and the
input in `seeds/` under a name that says what it was.
