# `terrace-contract`: moving the chart-side toolchain here

A plan for lifting the configuration-contract **consumer** out of
[helm-charts](https://github.com/TimSchoenle/helm-charts) and into this repository, as a shareable
binary rather than a directory of scripts one repository can import.

[config-contract-plan.md](config-contract-plan.md) designed the producer and sketched the chart
side. Both were built. This is the plan for the half that was built in the wrong place.

---

## 0. The decision

One new workspace member, `contract/`, publishing the package `terrace-contract` and one binary of
the same name. It absorbs every rule in `helm-charts/.github/scripts` that reads a contract —
including the Helm-shaped ones — and is distributed as pinned release binaries, a composite action
in `TimSchoenle/actions`, and a Helm plugin. `helm-charts` keeps its charts, its templates and its
`just` recipes; it stops keeping an implementation of this format.

Three things move that are not obviously ours, and section 5 is about why they come anyway and
what stops them contaminating `spec/v1/`. A second producer is being written in Java while
this is being read, and section 6 is what that changes — including three rules the Python has
wrong today, which the port must fix rather than carry across.

---

## 1. Why here

This repository already owns three of the four things involved. It defines the document
([`spec/v1/FORMAT.md`](../spec/v1/FORMAT.md)), it holds a producer to that definition
([`src/schema/`](../src/schema/)), and it states in three tiers what conformance means
([`spec/v1/CONFORMANCE.md`](../spec/v1/CONFORMANCE.md)). What it does not have is a reader — and
the only reader that exists is 15,000 lines of Python in a chart repository's `.github/scripts`.

That placement costs four things, and each of them is already being paid:

**The normative rules have two homes.** `config_contract.py`'s docstring says its classification
order "is copied from `External`'s own documentation" and that a consumer getting it wrong
"disagrees with the loader about whether a deployment boots". A rule copied is a rule that drifts;
`tests/contract_read.rs` exists in this repository precisely because three rules had already
become wrong in a diff that never touched them. There is one such test here and none there.

**Nothing else can use it.** A second chart repository, a Kustomize tree, an operator wanting to
know why a value is ignored — none of them can `import config_contract`. The format was made
multi-producer by `spec/v1/`; it is still single-consumer.

**Conformance is one-sided.** The corpus in `spec/v1/conformance/` holds producers. A consumer that
misreads `unreachable: indirection` as "skip this key" passes everything, and that is the exact
misreading `FORMAT.md` warns about.

**The chart repository pays for it.** `just check` currently needs `python3` with PyYAML, a pinned
`jv` binary for JSON Schema, and — for the refresh — `oras` and `cosign`. The first two exist only
for this pipeline. A Rust binary carrying `jsonschema` retires both.

What moving buys, concretely: one artefact instead of a directory; a reader held to the same
corpus as the writer; `lint-python` and `jv` deleted from `helm-charts`; and a second consumer
repository costing a `uses:` line instead of a copy.

---

## 2. The inventory

Everything under `helm-charts/.github/scripts` that reads a contract. Line counts are today's.

### Pure format — no Kubernetes, no Helm

| Source | Lines | Lands as |
|---|---|---|
| `config_contract.py` | 1030 | `document`, `union`, `classify`, `value` |
| `config_diff.py` | 1025 | `diff` |
| `config_shapes.py` (extraction half) | ~600 of 1396 | `shape` |
| `config_testgen.py` (probe half) | ~700 of 1134 | `probe` |
| `config_secrets.py` (inventory half) | ~300 of 765 | `secrets::inventory` |
| `config_paths.py` | 59 | dissolved into `helm::tree` |

### Kubernetes — objects, not charts

| Source | Lines | Lands as |
|---|---|---|
| `config_manifests.py` | 160 | `k8s` |
| `config_gate_document.py` | 204 | `gate::document` |
| `config_gate_container.py` | 402 | `gate::container` |
| `config_report.py` | 103 | `report` |

### Helm — the vocabulary this repository does not have today

| Source | Lines | Lands as |
|---|---|---|
| `config_declaration.py` | 816 | `helm::declaration` |
| `config_bindings.py` | 813 | `helm::bindings` |
| `config_shapes.py` (block half) | ~800 | `helm::schema_block` |
| `config_readme.py` | 322 | `helm::readme` |
| `config_coverage.py` | 179 | `helm::coverage` |
| `config_scaffold.py` | 1458 | `helm::scaffold` |
| `config_testgen.py` (suite half) | ~430 | `helm::unittest` |
| `config_secrets.py` (reconcile half) | ~465 | `helm::secrets` |

### Entry points — become subcommands, not modules

`check-config.py` (312), `check-config-bindings.py` (524), `contract-diff.py` (360),
`refresh-contracts.py` (575), `generate-contract-tests.py` (798), `adopt-config.py` (1145),
`explain-config.py` (1034), `config-secrets.py` (355). Their loops, argument parsing and exit
statuses are replaced by `clap` and `report`; their rules are already in the modules above.

### Tests and recipes

`tests/test_contract_*.py` — 8,093 lines across thirteen files — port to `contract/tests/`.
`just/contracts.just` — 825 lines — shrinks to nineteen one-line wrappers.

**Total: ~15.0k lines of implementation, ~8.1k of tests, 825 of `just`.** Expect the Rust to land
smaller on the rules and larger on the plumbing, because `clap` replaces eight `argparse` blocks
and one error type replaces eight conventions for reporting.

### Explicitly not moving

`add-tunable.py`, `audit-observability.py`, `chart-index.py`, `check-chassis.py`,
`check-immutable-fields.py`, `check-values-docs.py`, `crd-schema-refs.py`,
`extract-prometheus-rules.py`, `kube-schema-refs.py`, `kube-version-floor.py`, `rule_anchors.py`,
`schema-presets.py`, and the non-configuration half of `new-chart.py`. None of them reads a
contract. `just render`, `helm`, `kubeconform`, `kube-linter` and `helm unittest` stay where they
are and keep being invoked by `just`.

---

## 3. Crate layout

```
contract/
  Cargo.toml            package terrace-contract, [[bin]] terrace-contract
  src/
    lib.rs
    document.rs         the envelope, read tolerantly
    union.rs            several images, one document
    classify.rs         the ordered variable classification
    value.rs            check_text, then check_parsed if the loader is known (§6.1)
    conform.rs          hold a document to a conformance tier
    shape.rs            a key's element schema, composed through items/additionalProperties
    probe.rs            a value a key accepts that no chart produces by accident
    diff.rs             findings between two documents, and their impact
    secrets.rs          the credential inventory
    report.rs           findings, severity, text/JSON/step-summary rendering
    k8s/                manifests, workloads, containers, mounts, projected file names
    gate/               document, container, requiredness
    helm/               declaration, bindings, schema_block, readme, coverage,
                        scaffold, unittest, secrets, tree
    oci.rs              resolve, verify, discover, fetch — by delegation (§7.4)
    bin/main.rs         argument parsing and exit codes, and nothing else
  tests/                the ported suites, over the ported corpus
```

### Why a member and not a feature

`schema-cli` is a feature of this crate because its caller is the service's own `main`, compiled
from the service's own types. `terrace-contract` has the opposite shape: nothing links it, it runs
after the build, and it needs `clap`, `jsonschema`, a YAML stack and `regex` — four dependencies a
service loading a config file at boot has no business compiling. The manifest's existing argument
for splitting `loader` from `reload` ("a service that only reads a config file at boot has no
reason to link tokio, notify and tracing") is the same argument one level out.

`macros/` already establishes that a member is not a workspace of its own here: one `Cargo.lock`,
one `cargo fmt --all`, one `cargo clippy --workspace`, one version. `terrace-contract` joins on
those terms — version-locked to `terrace-config`, `publish = false`, workspace lints inherited, so
`unsafe_code = "forbid"` and `missing_docs` hold for it too.

### The lib/bin split is load-bearing

Every rule lives in the lib and returns findings; the bin decides where they print and what the
exit status is. That is the split `check-config.py` and `config_gate_*.py` already draw, and it is
what lets a test construct a container dict and a contract and read the list back. It is also what
makes the crate usable as a library by a repository that wants the gates and not the CLI.

### Cargo features

| Feature | Contents | Default |
|---|---|---|
| `k8s` | manifests + gates 1–3 + report | yes |
| `helm` | the declaration, the markers, the generators | yes |
| `oci` | `pull`, and the delegation to `oras`/`cosign` | yes |
| `cli` | `clap`, the binary | yes |

`--no-default-features` leaves the format core: read a document, union, classify, check a value,
diff. That is what a consumer embedding this in something that is not Kubernetes wants, and
keeping it compiling is what stops Helm vocabulary leaking down into the reader.

### Dependency budget

| Crate | For | Note |
|---|---|---|
| `serde`, `serde_json` | documents | already here |
| `jsonschema` | gate 1 | already a dev-dependency at `=0.55.0`, `default-features = false` — the pin is known-good and refuses remote `$ref`s, which is exactly the posture `jv` was chosen for |
| `toml` | the rendered document | `figment` pulls it already; take it directly |
| `clap` | the binary | `derive`, behind `cli` |
| `regex` | `ignore` patterns, README and Dockerfile block anchors | |
| a YAML pair | see below | the one open technical question |
| `thiserror` / `anyhow` | errors | `thiserror` in the lib as the crate already does, `anyhow` in the bin |

No OCI client crate. See §7.4.

### The open question: YAML with comments

Three of the Helm modules do not read YAML — they read **comments inside YAML**. A `@config`
marker lives in a `# @schema` block as `# # @config projection telemetry.sentry_dsn optional`;
`helm-schema` blocks are generated into comments; `adopt` and `scaffold` write values into a file
whose comments must survive. PyYAML does not preserve comments either, which is why the Python
does structure with PyYAML and markers with a line scanner keyed on line numbers.

The Rust equivalent needs the same two halves, and the second one decides a dependency:

- typed reads of `config-contract.yaml` and `values.yaml` — `serde_yaml` is unmaintained and
  `deny.toml` will say so; `serde_norway` or `serde_yaml_ng` are the live forks.
- the marker and block work — `yaml-rust2` or `saphyr` give events with markers (line, column),
  which is what a line-keyed scanner needs, and `marked-yaml` gives a tree with spans.

**Spike this first, before anything else in phase 3.** It is the only piece of the port where the
Python approach does not transfer directly, and picking wrong means rewriting three modules.

---

## 4. The command surface

Nineteen `just` recipes become one binary. The mapping is deliberately not one-to-one: every
`x` / `check-x` pair collapses into one subcommand with `--check`, so the writer and the checker
are one code path and cannot disagree about what "already correct" means.

| Today | Tomorrow |
|---|---|
| `just contracts` | `terrace-contract pull` |
| `just check-contracts` | `terrace-contract pull --check` |
| `just check-config` | `terrace-contract check --manifests rendered` |
| `just check-contract-coverage` | `terrace-contract coverage` |
| `just check-config-bindings` | `terrace-contract bindings --check` |
| `just adopt-config` | `terrace-contract bindings --adopt` |
| `just config-shapes` / `check-config-shapes` | `terrace-contract shapes` / `--check` |
| `just config-readme` / `check-config-readme` | `terrace-contract readme` / `--check` |
| `just contract-tests` / `check-contract-tests` | `terrace-contract tests` / `--check` |
| `just config-secrets` | `terrace-contract secrets` |
| `just check-config-secrets` | `terrace-contract secrets --reconcile rendered` |
| `just contract-diff` | `terrace-contract diff --since origin/main` |
| `just explain` | `terrace-contract explain <chart> [pattern]` |
| `just test-contract-union` | deleted — it becomes `contract/tests/union.rs` |
| `new-chart.py`'s config half | `terrace-contract scaffold` |
| `just sync-config` | `terrace-contract sync` (`pull`, `shapes`, `readme`, `tests`) |

Global: `--charts <dir>` (default `charts`), `--format text|json|github`, `--chart <name>` to
narrow. Exit `0` clean, `1` findings, `2` usage or IO. Two more the Python never had because a
script directory cannot offer them: `terrace-contract completions <shell>` and
`terrace-contract validate <file>` — validate any contract against `spec/v1/contract.schema.json`,
useful to a producer in another language with no Rust toolchain; and
`terrace-contract conform --tier <n>`, which is what a second producer's own build runs (§6.5).

One subcommand is new and belongs here rather than in `schema-cli`: `terrace-contract image
verify`, which is [`schema::cli::verify`](../src/schema/cli/verify.rs) reachable without writing a
Rust binary. The library half stays where it is — a service's own test uses it — and the CLI
becomes the way a CI step with an image and no toolchain calls it.

### What it deliberately does not do

- **Write `Chart.yaml`.** `config_diff.py`'s severity table "is a suggestion with its reasons
  attached, never an edit", and the reasoning holds: only the reviewer knows whether the chart
  writes the key that moved.
- **Run `helm template`.** `just render` carries the CRD `--api-versions` and the retry around
  network `$ref`s, and the whole design depends on the gates reading byte-identical manifests to
  the ones kubeconform sees. The CLI takes a rendered directory; it never produces one.
- **Reach a network, except in `pull`.** One networked subcommand, as today one networked recipe.

---

## 5. Helm vocabulary in this repository

Scope C imports three vocabularies that are not the document format: the chart declaration
(`config-contract.yaml`), the `@config` marker language, and the round-trip enrolment file. They
are a chart repository's conventions, and they are arriving in the repository that publishes a
language-neutral wire format. Two things keep that from being a mistake.

**They do not enter `spec/v1/`.** That directory is the envelope, and its `$id` is a promise:
"`spec/v1/` describes `terrace_contract: 1` and will not be edited to describe anything else". A
Helm convention has no business inside a document format a Java producer conforms to. They land in
a new, independently versioned tree:

```
spec/helm/v1/
  DECLARATION.md          what config-contract.yaml means
  declaration.schema.json validates one, and ships with the binary
  MARKERS.md              the @config grammar, the five refusals, where a marker may sit
  ENROLMENT.md            the round-trip test enrolment
```

`terrace-contract validate --declaration charts/x/config-contract.yaml` then checks a chart's own
file against a published schema — a gate `check-config.py` cannot offer today, because the shape
lives only in `config_declaration.py`'s parser.

**They sit behind the `helm` feature**, above `k8s`, which is above the format core. The
dependency arrow only ever points down. A repository that renders Kubernetes objects some other
way gets `k8s` and the three gates without a word about charts, and the fact that
`--no-default-features` must keep compiling is the mechanical check that no Helm concept has
leaked into `document` or `classify`.

---

## 6. A second producer

A Java implementation is being written now. It changes what a consumer is allowed to assume, and
three of the changes are not additive: the Python gets them wrong today, invisibly, because there
has only ever been one producer. The port must fix them rather than carry them across, and the
fixes are cheap now and expensive after the first Java-produced contract is vendored.

### 6.1 The reads belong to the loader, and the consumer must ask

`FORMAT.md` is unambiguous: the reads it tabulates "are normative for `producer.loader ==
"figment"` and for nothing else. A consumer meeting a loader it does not know MUST skip step 2
and say so."

`config_gate_container.py:189-192` calls `check_text` and then `check_parsed` unconditionally, and
`config_contract.py` never reads `producer` at all — the field does not appear in the module.
Against a Spring-binder document vendored beside a figment one, that applies figment's measured
patterns to text Spring reads differently, and it fails in the expensive direction: a pattern that
refuses text the loader accepts stops a deployment that was correct, which is the exact failure
`FORMAT.md`'s superset rule exists to prevent.

So `value` is not a pair of functions; it is a registry.

```rust
/// One loader's environment reads, as measured against that loader.
pub trait Reads {
    fn parse(&self, form: TextForm, text: &str) -> Option<Json>;
}

/// `None` means: skip step 2, and report it as skipped rather than passed.
pub fn reads_for(loader: &str) -> Option<&'static dyn Reads>;
```

`figment` is built in and already pinned by [`tests/contract_read.rs`](../tests/contract_read.rs).
The Java binder's table lands here in the same change that adds it to `FORMAT.md`, which
`CONFORMANCE.md` already requires of a tier 1 implementation. An unknown loader yields a finding at
warning severity — `report`'s existing distinction between "found wrong" and "could not check" is
exactly the right shape for it and needs no new concept.

### 6.2 Tier 2 is an assumption, and one rule already makes it silently

`Union.structured_parent` decides that a name extending a `structured` key's spelling by
`dialect.nesting_separator` addresses a leaf of that map. That is true of figment. It is not true
of a binder with its own relaxed-binding rules, which is precisely the case `CONFORMANCE.md`
predicts: "An implementation that hands naming to a binder with its own relaxed-binding rules will
not reach tier 2 without overriding it, and should not pretend to."

The port's rule, stated so it can be checked rather than remembered: **derive nothing the document
states.** Everything that reads `env`, `env_file`, `secrets_file` and `unreachable` as published is
tier 1 work and stays unconditional — which is most of the toolchain, and is why a tier 1 Java
producer is usable by the gates on the day it emits its first document. Everything that *derives* a
spelling sits behind the registry in §6.1. Mechanically: `grep -r nesting_separator contract/src`
must return hits only in `value`, `conform` and `diff`.

### 6.3 `ty` is not a type name

`FORMAT.md` says so already, and the Python is nearly clean by accident — two uses, a display set
in `config_shapes.py` and a severity row in `config_diff.py`, neither of which switches behaviour
on the value. Neither may start. `shape` and `probe` read `constraint`, `text_form` and
`text_constraint`; a `java.time.Duration` has to reach them exactly as `u64` does, and the moment
one `match` arm spells a Rust type name the crate has quietly become single-producer again.

### 6.4 `schema_version: 1` is not hypothetical

A new producer implements the simple thing first, and the one non-Rust document in the corpus today
— `foreign-dialect.json` — is `schema_version: 1`, the version before `constraint` learned to nest.
`shape` must degrade to "no element schema published" rather than assume `items`, and the
generators must emit a narrower `@schema` block rather than refuse to emit one.

That fixture is also **not valid against
[`spec/v1/contract.schema.json`](../spec/v1/contract.schema.json)**: it carries no `producer` block
and the schema requires one. The Python never noticed, because it never reads the field. Correcting
it belongs to phase 1 — and a hand-written fixture that drifted from the published schema is itself
the argument for `terrace-contract validate` existing.

### 6.5 What the Java build needs from this binary

The Java producer's build has the same two questions the Rust one does, and neither answer needs a
Rust toolchain:

- **is what I emitted a valid contract, at the tier I claim?** — `terrace-contract conform --tier 1
  contract.json`. Validate against `spec/v1/contract.schema.json`, then assert every MUST in
  `FORMAT.md` checkable from one document: the eight refusals, a `producer` block naming a loader,
  `unreachable` set wherever `env` is null, no external variable reaching into the loader's
  namespace. `--tier 2` adds the re-derivation of every spelling from the dialect and compares.
- **did the image I built actually carry it?** — `terrace-contract image verify`, which reads
  labels and a Dockerfile and knows nothing about any language.

That is the whole of the Java side's dependency on this repository: one pinned binary, from the
same release as §7.1. A Gradle plugin wrapping it is worth doing if the Java side asks for it, on
the same terms as the Helm plugin — a thin wrapper, a third channel, not the first.

It also settles a question §7.5 would otherwise leave awkward. The binary's version is this
crate's, and this crate is the Rust producer; a Java shop pinning `terrace-contract v0.12.0` should
not be pinning "the Rust implementation". So the CLI gates on the **envelope** — `terrace_contract`
and `schema.schema_version` — never on `producer.version`, and `terrace-contract --version` prints
the envelope versions it reads alongside its own. Lockstep versioning stays; what it means is
stated.

### 6.6 The corpus grows on the consumer side

`spec/v1/conformance/` holds three producer cases. A second producer means it must also hold the
documents a *consumer* has to survive, and none of them exist:

| Case | Asserts |
|---|---|
| `foreign-loader/` | step 2 is skipped and reported, not silently performed |
| `schema-version-1/` | `shape` degrades and the generators narrow, rather than either failing |
| `tier-1-spellings/` | no rule re-derives a spelling the document states |
| `foreign-ty/` | a type vocabulary from another language changes no behaviour |

They are cheap to write, they are checkable before any Java image exists, and they are what catches
the Java integration breaking months before there is a contract to vendor. Write them in phase 1,
against the reader, while the reader is the only thing that exists.

---

## 7. Distribution

### 7.1 Release binaries — the primary channel

A new `.github/workflows/release-binaries.yml`, triggered by the tag release-please pushes.
Cross-compiles, attaches archives plus `SHA256SUMS` to the GitHub release, and signs keyless with
cosign — which matters here more than usual, because `helm-charts` already refuses a *contract*
whose signer identity does not match `contract_signer`, and a tool that validates signed documents
arriving unsigned is a hole in the same argument.

| Target | Why |
|---|---|
| `x86_64-unknown-linux-musl` | GitHub runners; static, so no glibc floor |
| `aarch64-unknown-linux-musl` | ARM runners |
| `x86_64-pc-windows-msvc` | the Git Bash shell this toolchain is developed on |
| `aarch64-apple-darwin` | local |

`helm-charts` then pins a version in its `justfile` beside `jv`, `oras` and `cosign` and installs
by release URL. That is the pattern the repository already chose, for the stated reason that a
pinned single binary "runs identically in a Git Bash shell and keeps a `pip install` out of a
recipe" — and this replaces two of the three tools it was chosen for.

### 7.2 Composite action

`TimSchoenle/actions/actions/terrace-contract/install`: takes a version, resolves the platform,
verifies the checksum and the cosign signature, caches, and puts the binary on `PATH`. Every job
in `helm-charts` that today installs `jv` by release URL becomes one `uses:`. Renovate already
tracks that repository's action tags, so the version bump arrives the way every other one does.

### 7.3 Helm plugin

`plugin.yaml` with an install hook downloading the same release asset, so
`helm plugin install https://github.com/TimSchoenle/terrace-config` gives an operator
`helm terrace-contract explain portfolio 'sentry.*'` without a checkout. Thin by construction — it
is the same binary under a different name — and worth exactly what that costs, which is a
`plugin.yaml`, an install hook and a row in the release matrix. It is the third channel, not the
first: CI uses §7.1 and §7.2.

### 7.4 The registry, by delegation

`pull` shells out to `oras` and `cosign` rather than linking an OCI client. Three reasons, in
order of weight: the signature **policy** — which workflow identity a contract must be signed by —
is the consuming organisation's, not this crate's, and a linked verifier would have to grow an
option for it; `helm-charts` already pins both binaries; and an OCI client plus a TLS stack is a
large dependency and a large attack surface for one subcommand. The delegation sits behind a trait
so a future `--feature oci-native` can supply the other implementation without moving the rules.

### 7.5 Versioning

release-please's config declares one package at `.` and `include-component-in-tag: false`;
`macros/` is already version-locked at `0.11.0` under it. `terrace-contract` joins on the same
terms: one version, one tag, one changelog, and the binary's `--version` is the crate's. That
keeps the contract format, the producer and the consumer on one number, which is the property that
makes "this chart was gated by `terrace-contract v0.12.0`" a statement about the format too.

The cost is real and worth stating: a chart-side bug fix now needs a release of this repository.
Mitigation is an escape hatch rather than a second version line — `helm-charts` recipes honour
`TERRACE_CONTRACT_BIN`, so a branch build can be pointed at without cutting a release, and that is
also what makes the parity harness in §8 possible.

---

## 8. Migration

Six phases plus a phase 0. Each ends with something deleted from `helm-charts` — a phase that adds
a Rust implementation and leaves the Python running is a phase that has doubled the number of
places the rule lives.

**Phase 0 — the parity harness.** Before any port. A script that runs both implementations over
the whole `helm-charts` tree and diffs their JSON output, per subcommand, gated in this
repository's CI against a checked-out `helm-charts` at a pinned ref. This is the single most
valuable de-risking step available: 15,000 lines of rules with 8,000 lines of tests and one real
corpus of nine charts and twenty-odd contracts, and the corpus is the part that finds what the
tests do not. Every later phase's exit criterion is "parity, or a documented deliberate
difference".

**Phase 1 — the format core.** `document`, `union`, `classify`, `value`, `conform`, `report`.
Port `test_contract_union.py` and `test_contract_values.py`. Add the consumer side of
`spec/v1/conformance/`: every case in the corpus is read back and asserted about, so a
misreading of `unreachable: indirection` fails here. Everything in §6 lands in this phase and
not later — the loader registry, the four consumer cases, the corrected fixture and
`conform` — because every one of them is a constraint on the reader's shape rather than a
feature on top of it, and retrofitting a registry to a call site that assumed figment is how the
Python arrived where it is. Nothing is deleted from `helm-charts` yet; this is the phase that
has no user.

**Phase 2 — the gates.** `k8s`, `gate::{document, container}`, and the `helm::declaration` reader
they need. Ship `terrace-contract check`. *Delete* `check-config.py`, `config_gate_*.py`,
`config_manifests.py`, `config_contract.py`, `config_report.py` and the `jv` pin. `just
check-config` becomes a wrapper. This is the phase that pays for phase 1.

**Phase 3 — the marker language.** The YAML spike, then `helm::bindings`, `helm::schema_block`,
`helm::coverage`. Ship `bindings`, `shapes`, `coverage`. Delete `config_bindings.py`,
`config_shapes.py`, `check-config-bindings.py`, `config_coverage.py`. Highest risk in the plan and
the reason the spike is first.

**Phase 4 — the derived documents.** `diff`, `secrets`, `helm::readme`, `probe`,
`helm::unittest`. Ship `diff`, `secrets`, `readme`, `tests`, `explain`. Delete
`config_diff.py`, `contract-diff.py`, `config_secrets.py`, `config-secrets.py`,
`config_readme.py`, `config_testgen.py`, `generate-contract-tests.py`, `explain-config.py`.

**Phase 5 — the writers.** `helm::scaffold` and the adopt path. Ship `scaffold` and
`bindings --adopt`. Delete `config_scaffold.py`, `adopt-config.py`, and `new-chart.py`'s
configuration half. Golden-file tests come across unchanged, because the output is bytes.

**Phase 6 — the network, and the cleanup.** `oci`, `pull`, `sync`. Delete `refresh-contracts.py`,
`config_paths.py`, `config_declaration.py`'s remainder, and `lint-python` if nothing else in the
repository needs it. `just/contracts.just` drops from 825 lines to a header plus wrappers — keep
the header, which is the best statement of why any of this exists.

Ordering rationale: the network is last because it is the one thing whose failure is not the
tool's fault, and the writers are late because a generator that is wrong writes its mistake into
the tree, where the parity harness cannot see it as a diff.

---

## 9. Testing

**The corpus moves first.** `helm-charts/.github/testdata/contracts/` — `api.json`,
`conflicting.json`, `deep.json`, `foreign-dialect.json`, `remote-ref.json`, `worker.json` — becomes
`contract/tests/fixtures/`. Two of those (`foreign-dialect`, `remote-ref`) are refusal cases and
belong in `spec/v1/conformance/` proper, alongside the four new consumer cases in §6.6 — and
`foreign-dialect.json` is corrected on the way in, since it does not validate against the
schema it claims to conform to.

**The 8,093 lines of Python tests port as integration tests**, not as unit tests inside modules:
they are already written as "construct a document and a manifest, call the rule, read the findings
back", which is an integration test in Rust's vocabulary and keeps the lib's public surface honest.

**The parity harness stays after the migration**, pointed at `helm-charts` `main`, as the
regression gate for the format core. It is the only test with a real corpus.

**`fuzz/` gains two targets**: the document reader, over arbitrary JSON, because it is now parsing
input written by a producer this repository does not control; and the marker scanner, over arbitrary YAML, because it is
line-keyed and line-keyed parsers are where panics live. The existing convention applies — the
oracles run under `cd fuzz && cargo test` without libFuzzer.

**`tests/contract_read.rs` gets a sibling.** That file implements the documented read and asserts
the loader agrees. Once `value.rs` exists, the assertion becomes three-way: the prose, the loader,
and the shipped reader. That is the check that stops this whole exercise from re-creating the
drift it exists to remove.

---

## 10. What it costs this repository

`cargo build` for a service is unchanged: nothing in `terrace-config` depends on
`terrace-contract`, and the arrow must stay pointing that way — a lint-level rule for review, not
something cargo enforces.

CI grows: `clippy` and `features` matrices gain `-p terrace-contract` rows including
`--no-default-features` (the check that keeps Helm out of the core), `test` already passes
`--workspace`, `deny` now judges `clap`, `jsonschema`, `regex` and the YAML pair, and `doc` gains a
row. A new `release-binaries` workflow, and a `parity` workflow that checks out `helm-charts`.

MSRV stays 1.94 unless a dependency raises it; that is a constraint on the YAML choice.

`README.md` is generated from `.github/templates/README.md.hbs` via
`.github/scripts/readme-variables.sh` — an install snippet for the binary and a row in the feature
table are template edits, not README edits.

---

## 11. Risks

| Risk | Answer |
|---|---|
| The Python changes mid-port | Freeze `.github/scripts/config_*.py` to bug fixes once phase 2 starts, and let the parity harness catch what lands anyway |
| Comment-preserving YAML has no good crate | Spike before phase 3; the fallback is the Python's own approach — structural parse plus a line-keyed scanner — which is known to work and is not elegant |
| A chart-side fix needs a release here | `TERRACE_CONTRACT_BIN` override, plus release-please's existing cadence; if it bites more than twice, that is the evidence for splitting the version line |
| Helm concepts erode the format core | `--no-default-features` in the features matrix, and `spec/helm/v1/` kept out of `spec/v1/` |
| A generator writes a wrong file the diff cannot see | Phase 5 is late, and every generator ships `--check` from the same code path |
| Windows Git Bash | It is the development shell; `x86_64-pc-windows-msvc` is in the matrix from the first release, and the parity harness runs there |
| The Java producer lands mid-port | Everything it needs is phase 1 (§6): the registry, `conform`, and four corpus cases. A tier 1 document is readable by the gates with no further work |
| A rule quietly assumes figment again | `grep -r nesting_separator contract/src` in CI, and `reads_for` returning `Option` rather than a default — the type makes the skip unforgettable |
| Scope | 23,000 lines is a quarter's work at a steady pace. The phases are independently shippable and each one deletes Python; a stop after phase 2 still leaves the repository better than it is |

---

## 12. Open questions

1. **The YAML pair.** §3. Blocks phase 3, not phases 0–2.
2. **`spec/helm/v1/` here, or in `helm-charts`?** The argument for here is that the binary
   validates against it. The argument for there is that it describes that repository's
   conventions and no format. Written above as "here"; worth one more look before phase 3.
3. **Does `terrace-contract` absorb `schema-cli`?** They are different directions — one needs the
   Rust types, the other an image — so no. But `image verify` overlaps, and the overlap should be
   one implementation in the library with two callers.
4. **Does the parity harness need `helm-charts` checked out, or a vendored fixture tree?**
   Checked out finds more; vendored is reproducible. Start checked out at a pinned ref.
5. **Where does the Java binder's read table live?** `FORMAT.md` under a heading of its own is
   what `CONFORMANCE.md` already mandates. Confirm the Java side agrees before it ships a
   document, because the table is what makes its `text_constraint` fields actionable rather
   than decorative.
6. **Does the Java side want a Gradle plugin, or is a pinned binary enough?** §6.5. Ask them;
   do not build it speculatively.
