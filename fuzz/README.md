# Fuzzing

Three oracles over the loader, reachable two ways.

## Without libFuzzer, on any toolchain

```bash
cd fuzz && cargo test
```

Replays every committed seed — and everything a campaign promoted into `corpus/` — through the
matching oracle, then runs a deterministic generated sweep over the tokens that sit on a rule
boundary. This is what CI gates on, and it is why the oracle bodies live in `src/oracle/` rather
than in the target binaries: `cargo fuzz` needs a nightly-only sanitizer that on Windows also
needs a runtime shipping with Visual Studio, and an oracle that only runs under one toolchain is
an oracle nobody checks.

`TERRACE_FUZZ_ITERATIONS` sets the sweep budget per oracle; it defaults to 600 so the suite stays
a few seconds, and CI raises it.

Both bugs found while writing these were found this way, before a campaign ever ran — one in the
crate ([`Dialect::is_reserved`](../src/dialect.rs) comparing case-sensitively) and one in the
harness.

## With libFuzzer

Run from the **repository root**, not from here.

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run env_load    fuzz/corpus/env_load    fuzz/seeds/env_load    -- -dict=fuzz/dictionaries/config.dict
cargo +nightly fuzz run secrets_dir fuzz/corpus/secrets_dir fuzz/seeds/secrets_dir -- -dict=fuzz/dictionaries/config.dict
cargo +nightly fuzz run toml_layers fuzz/corpus/toml_layers fuzz/seeds/toml_layers -- -dict=fuzz/dictionaries/config.dict
```

`+nightly` is explicit because `cargo fuzz` runs from the root, where this directory's
`rust-toolchain.toml` does not apply. `libfuzzer-sys` compiles the crate under test with
`-Z sanitizer=address`, which is nightly-only.

The **first** directory on the command line is the one libFuzzer writes newly-discovered inputs
into, which is why `corpus/` comes before `seeds/`: `seeds/` is committed and only ever read.

## The targets

| Target | Layer | The claim it defends |
|---|---|---|
| `env_load` | `TEST_*` environment variables | boot and reload extract the same values, and an unchanged environment is not a change |
| `secrets_dir` | `$TEST_SECRETS_DIR` and `TEST_<KEY>_FILE` | a key supplied twice is never silently accepted; a value loses trailing line terminators and nothing else |
| `toml_layers` | `$TEST_CONFIG` as a directory of fragments | `..data` and non-`*.toml` entries never contribute; later names win |
| `kube` | the Kubernetes stamp a chart puts on a rendered object | every key and value it emits is legal object metadata, and an unpinned image reference is refused rather than stamped |

Each target computes, independently of the loader, what the input it built *should* produce, then
asserts it. Totality — `Ok` or a typed `Error`, never a panic — is the floor, not the point: a
loader that returns the wrong configuration without crashing is the failure mode these are aimed
at, because it is the one that reaches production. Each target's module doc states its oracle.

## Input shape

Readable text rather than a derived `Arbitrary` struct, so seeds stay greppable and a crash
artefact can be read without a decoder. One directive per line; anything that does not parse is
skipped, so a mutation that corrupts one line still exercises the rest.

```text
f:<name>=<content>     a file in the secrets directory       (secrets_dir)
t:<name>=<content>     a fragment in the config directory    (toml_layers)
p:<SUFFIX>=<content>   a TEST_<SUFFIX>_FILE indirection      (secrets_dir)
e:<SUFFIX>=<value>     the environment variable TEST_<SUFFIX>
i:<reference>          an image that reads the document      (kube)
d:<key>                the ConfigMap data key it is          (kube)
```

`\n`, `\r` and `\\` are decoded in file bodies, so the trailing-terminator rule — the one rule
about *which* bytes a value loses — is reachable from a line-oriented grammar.

## Containment

Two rules in `fuzz_targets/support.rs` exist to keep a campaign inside its temporary directory,
and a fuzzer that reaches either is reporting a harness bug rather than a crate bug:

- **File names are filtered.** Path separators, `..`, drive-letter colons, the Windows device
  names and trailing dots or spaces are refused. Everything else, Unicode included, is allowed
  through — a `Secret` key really can be any of it.
- **Indirection paths are never taken from the input.** `TEST_<KEY>_FILE` holds a *path*; a
  fuzzer-chosen one would have the target reading arbitrary files on the host. The target names
  the file and the input supplies only its contents.

`env_load` additionally filters out `TEST_CONFIG`, `TEST_SECRETS_DIR` and anything ending
`_FILE`, so a crash it finds always reproduces from the input alone rather than from a file
elsewhere on the machine.

## What is not fuzzed

The `reload` supervisor. Its inputs are a filesystem event and a caller's closure, neither of
which a byte string models; `tests/reload.rs` drives it against a real watcher instead.

Non-UTF-8 file contents. The targets take `&str`, so the "not valid UTF-8" branch is covered by
unit tests rather than here.

## Findings

A reproducer goes into `seeds/<target>/` under a descriptive name, where `cargo test` replays it
forever. If it pins a rule worth stating in prose too, it also becomes a named regression test in
[`../tests/`](../tests) or a unit test beside the code — `reserving_ignores_case_in_both_directions`
in `src/dialect.rs` came from the `secrets_dir` sweep. Crash blobs under `artifacts/` are not
committed.

### Fixed

- **`Dialect::is_reserved` compared reserved names case-sensitively.** `SecretsDir` upper-cases a
  file name before the check and `FileSuffixEnv` had nothing to upper-case, so `TEST_profile_FILE`
  could supply a key that a secrets-directory file named `profile` was refused. On Windows, where
  environment names are case-insensitive, those are the *same variable* — so which one you got
  depended on how the operator typed it. The check now folds case.
