# `terrace-contract`: one toolchain for every implementation

A plan for a single binary that is everything the configuration contract needs *after* a document
exists — the build-time renderings, the image wiring, and the chart-side gates — so that adding an
implementation means implementing one thing rather than nine.

Supersedes the first draft of this file, which planned only the chart-side half and assumed one
producer. [#98](https://github.com/TimSchoenle/terrace-config/pull/98) made that assumption false.

**This plan assumes #98 has landed.** Every path below is the post-#98 tree: `spec/`, `rust/`,
`java/`, `docs/`. Nothing here needs to wait for it, and phase 0 can start against either tree.

---

## 0. The decision

One new top-level directory, `cli/`, its own Cargo workspace, publishing the package and binary
`terrace-contract`. It depends on no implementation and versions on its own tag.

It absorbs three bodies of work that are today in three places and are all the same work:

- `rust/src/schema/cli/` — the nine renderings, the `--format` vocabulary, the label and Dockerfile
  blocks, the image read-back. Currently a Rust library feature, reachable only from a Rust build.
- `java/terrace-config-core/src/main/java/.../schema/` — the same renderings, being written again
  in Java right now, about 40% done.
- `helm-charts/.github/scripts/` — ~15,000 lines of Python that reads contracts and gates charts,
  importable by nothing and held to no corpus.

What each language keeps is one thing: **its own types, to a contract document.** Everything after
that document exists is the binary.

---

## Status

Phase 0 and most of phase 1 are built, in `cli/`. What exists and is under test:

| | |
|---|---|
| `document` | the envelope, read tolerantly, gating on `terrace_contract` before believing anything |
| `render` | **all seven renderings**, each byte-identical to the Rust implementation's over the whole corpus |
| `conform` | the eight refusals, and tier 2's spelling derivation |
| `validate` | any document against the embedded `spec/v1/contract.schema.json` |
| `image` | the label comparison and the Dockerfile block reader |
| `stamp` | build identity onto a document, with the round trip that makes it safe |
| tests | 26 property tests over hand-built documents — one per refusal, the tier 2 boundaries, a foreign `ty`, an integer past TOML's range — plus `cli/fuzz/`: three oracles that replay a committed corpus and a fixed-seed sweep on a plain `cargo test` |
| CI | `fmt`, `clippy` in both feature sets, the corpus tests, the fuzz oracles, and the assertion that `cli/` depends on no implementation |

`spec/v1/conformance/<case>/rendered/` landed with it: seven files per case, blessed by the Rust
implementation's own `TERRACE_SPEC_BLESS` run, which is §5 arriving with phase 1 rather than after
it.

Still to come, in the order §11 gives: the Java cutover (phase 2), then the chart half — `k8s`,
the gates, the marker language, the derived documents and the writers.

Two things found while building, both now fixed in the plan's own terms rather than only in code:

- **The TOML rendering's placeholder was `ty`-keyed** in the implementation it was ported from,
  which §7.3 says a shared renderer must never be. It reads the published `constraint` instead —
  the same answer, arrived at by the producer that did have the type, and right for every producer.
- **The fuzz oracles found three defects in themselves** before they found any in the crate, which
  is what §12 means by an oracle needing validating: a column check comparing the loader table's
  four columns against the key table's six, an agreement check that string-matched the validator's
  error text and so fired on unrelated documents, and an assertion about structured keys that no
  rule anywhere makes. All three would have been noise in a suite people learn to ignore.
- **`serde_json`'s `preserve_order` was wrong.** A producer builds its JSON with `serde_json::Map`,
  a `BTreeMap`, so every object in a published document is alphabetical — the JSON Schema's
  `properties` included. Sorting the same way is what makes `--format contract` a byte-for-byte
  round trip.

---

## 1. Why the first plan no longer fits

The first draft was right about the chart half and wrong about the shape, because it was written
against a repository with one producer. Three things changed.

**There are three implementations now, not one.** `rust/`, `java/terrace-config-loader` (vanilla,
targeting tier 2), and `java/terrace-config-spring-boot` (Spring's `Binder`, targeting tier 1).
A rule that lives in one of them is not shared; it is copied.

**The duplication has already started, and it is not the chart half.** `java/`'s core carries
`MarkdownRenderer` (109 lines), `TomlExampleRenderer` (381), `JsonSchemaRenderer` (212), `Column`
(169), `Node` (108), `Docs` (58) and `ContractValidator` (155) — 1,192 lines re-deriving what
`rust/src/schema/` already does. `java/README.md` states the rest plainly:

> the remaining renderings (markdown, toml, contract, labels, dockerfile, ...) are open

So the second implementation of the renderers is about 40% written and the third has not started.
This is the last cheap moment to decide there is one.

**The renderings are not implementation work.** Every one of them is a pure function of the
document. `rust/docs/CONTRACT.md` says so about the schema half already — "the other five are
derivable from it by a consumer who wants them" — and the contract embeds the schema, so the
contract is a superset of everything the nine formats need. Nothing in a markdown table or a TOML
skeleton needs Rust's types, figment, Jackson or Spring. They were in the producer because that is
where the data happened to be, not because that is where they belong.

The consequence is the whole plan: **a renderer that reads a document runs for every language,
including the ones that do not exist yet.**

---

## 2. The seam

```
per language, once                    the binary, for everyone
───────────────────────────────       ────────────────────────────────────────────────
types ──> Schema ──> Contract ──JSON──> render   --format markdown|toml|json-schema|…
                                        stamp    build identity onto a document
                                        conform  the eight refusals, and a tier
                                        image    labels and Dockerfile block, read back
                                        check    a rendered chart, against the document
                                        diff     what changed, and what it costs
                                        …
```

Everything left of the arrow needs the language's toolchain and can be nothing but per-language.
Everything right of it needs a JSON document and nothing else.

**A new implementation's obligation is therefore one sentence**: emit a document that passes
`terrace-contract conform --tier <n>`. Not nine renderers, not a validator, not a TCK. That is
what makes "any future language" a real claim rather than an aspiration — `spec/` already tells a
new producer what to emit, and this makes emitting it sufficient.

### What each side keeps

| | Rust | Java | A future language |
|---|---|---|---|
| types → descriptors | `Describe` derive | `terrace-config-processor` | its own |
| descriptors → `Contract` | `Schema::into_contract` | `ContractAssembler` | its own |
| `Contract` → JSON | `Contract::to_json` | `ContractCodec` | its own |
| the nine renderings | **library keeps them** (§5) | **never written** | never written |
| the eight refusals | fast path, better messages | fast path, better messages | optional |
| a TCK | — | wraps the binary | wraps the binary |

The one asymmetry is Rust's, and it is deliberate. `rust/`'s `schema` feature has a *runtime*
audience — a service rendering its own table into a log at boot — that a build-time binary cannot
serve. So the library keeps its renderers, and §5 is what stops them drifting from the binary's.
Java has no such audience and no such renderers; the four it has started are deleted rather than
finished.

---

## 3. Where it lives

```
spec/     the format, the conformance corpus, and (new) the rendered goldens
rust/     the Rust implementation — loader, producer, macros, fuzz
java/     the Java implementations — annotations, core, processor, loader, spring, tck
cli/      the shared toolchain. Its own Cargo workspace. Depends on no implementation.
docs/     cross-language documents, this one included
```

`cli/` is a sibling of `rust/`, not a member of it. That is the whole architectural claim in one
line of tree: **the binary is not part of the Rust implementation, it is written in Rust.** A
`cli/` nested under `rust/` would be read as Rust's tool by everyone who did not read this
document, and the Java side would be right to treat it that way.

Its own Cargo workspace and its own `Cargo.lock`, for the reason `rust/fuzz/` already gives for
being one: entirely different dependency weight, entirely different audience, and no reason for
one lock file to arbitrate between a library that must stay light and a binary that links `clap`
and a JSON Schema engine.

### Module layout

```
cli/
  Cargo.toml
  src/
    lib.rs
    document.rs      the envelope, read tolerantly
    render/          markdown, markdown-loader, markdown-keys, toml, json-schema,
                     labels, dockerfile — one module each, all from a document
    conform.rs       the eight refusals, and tier 1/2/3
    stamp.rs         build identity onto a document
    image.rs         labels_from_json, dockerfile_block, check_labels
    union.rs         several images, one document
    classify.rs      the ordered variable classification
    value.rs         check_text, then check_parsed if the loader is known (§7.1)
    shape.rs         a key's element schema, composed through items/additionalProperties
    probe.rs         a value a key accepts that no chart produces by accident
    diff.rs          findings between two documents, and their impact
    secrets.rs       the credential inventory
    report.rs        findings, severity, text/JSON/step-summary rendering
    k8s/             manifests, workloads, containers, mounts, projected file names
    gate/            document, container, requiredness
    helm/            declaration, bindings, schema_block, readme, coverage,
                     scaffold, unittest, secrets, tree
    oci.rs           resolve, verify, discover, fetch — by delegation (§9.5)
    bin/main.rs      argument parsing and exit codes, and nothing else
  tests/
```

Features stack so the arrow points one way and can be checked: `cli` → `helm` → `k8s` → `oci` →
core. `--no-default-features` leaves `document`, `render`, `conform`, `stamp`, `image`, `union`,
`classify`, `value`, `diff` — the producer-side half, which is what a language's build actually
needs and what must never learn what a chart is.

### Dependency budget

| Crate | For |
|---|---|
| `serde`, `serde_json` | documents |
| `jsonschema` (`=0.55`, `default-features = false`) | gate 1 and `validate`; already a known-good pin in `rust/`, and refuses remote `$ref`s |
| `toml` | the rendered document, and the TOML rendering |
| `clap` (`derive`) | the binary, behind `cli` |
| `regex` | ignore patterns, README and Dockerfile block anchors |
| a YAML pair | §3.1 — the one open technical question |
| `thiserror` / `anyhow` | lib / bin, as `rust/` already splits them |

No OCI client crate (§9.5). **No dependency on `terrace-config`**, and CI enforces it: a
`cargo tree` assertion, because the day that edge appears the claim in this section stops being
true and nothing else would notice.

### 3.1 The open question: YAML with comments

Three Helm modules do not read YAML — they read **comments inside YAML**. A `@config` marker lives
inside a `# @schema` block as `# # @config projection telemetry.sentry_dsn optional`; the schema
blocks are generated into comments; `adopt` and `scaffold` write values into a file whose comments
must survive. PyYAML does not preserve comments either, which is why the Python does structure
with PyYAML and markers with a line-keyed scanner.

The Rust equivalent needs both halves, and the second decides a dependency: `serde_yaml` is
unmaintained and `deny.toml` will say so, so typed reads go to `serde_norway` or `serde_yaml_ng`;
the marker work needs events with line and column, from `yaml-rust2`, `saphyr` or `marked-yaml`.

**Spike it before phase 4.** It blocks nothing earlier — the entire producer-side half, which is
what the Java work needs, touches no YAML at all.

---

## 4. The command surface

Two groups, because there are two audiences and only the first is new.

### Producer side — a build, in any language

| Command | Replaces |
|---|---|
| `terrace-contract render --format <f>` | `rust`'s `--format` for eight of nine formats; every renderer `java/` has written or planned |
| `terrace-contract stamp --version --revision --created` | `Request::stamp` |
| `terrace-contract conform --tier <n>` | `ContractValidator` + `terrace-config-spec-tck` as the normative copy |
| `terrace-contract validate` | nothing — new; any document against `spec/v1/contract.schema.json` |
| `terrace-contract image verify --labels … --dockerfile …` | `rust/src/schema/cli/verify.rs`, reachable without a Rust build |

Every one reads a contract document — or a bare schema document, which is `--format json`'s own
output — from a path or stdin, and writes to stdout. That is the entire interface. `--format
contract` stays with the producer, because producing the contract is the one thing the binary
cannot do.

### Consumer side — a chart repository, or any Kubernetes tree

| Today in `helm-charts` | Tomorrow |
|---|---|
| `just contracts` / `just check-contracts` | `terrace-contract pull` / `--check` |
| `just check-config` | `terrace-contract check --manifests rendered` |
| `just check-contract-coverage` | `terrace-contract coverage` |
| `just check-config-bindings` / `just adopt-config` | `terrace-contract bindings --check` / `--adopt` |
| `just config-shapes` / `check-config-shapes` | `terrace-contract shapes` / `--check` |
| `just config-readme` / `check-config-readme` | `terrace-contract readme` / `--check` |
| `just contract-tests` / `check-contract-tests` | `terrace-contract tests` / `--check` |
| `just config-secrets` / `check-config-secrets` | `terrace-contract secrets` / `--reconcile rendered` |
| `just contract-diff` | `terrace-contract diff --since origin/main` |
| `just explain` | `terrace-contract explain <chart> [pattern]` |
| `just test-contract-union` | deleted — becomes `cli/tests/union.rs` |
| `new-chart.py`'s config half | `terrace-contract scaffold` |
| `just sync-config` | `terrace-contract sync` |

Every `x` / `check-x` pair collapses into one subcommand with `--check`, so the writer and the
checker are one code path and cannot disagree about what "already correct" means.

Global: `--charts <dir>`, `--format text|json|github`, `--chart <name>`. Exit `0` clean, `1`
findings, `2` usage or IO. Plus `terrace-contract completions <shell>`.

### What it deliberately does not do

- **Produce a contract.** That needs the types. It is the seam.
- **Write `Chart.yaml`.** `config_diff.py`'s severity table "is a suggestion with its reasons
  attached, never an edit", and only the reviewer knows whether the chart writes the key that moved.
- **Run `helm template`.** `just render` carries the CRD `--api-versions` and the retry around
  network `$ref`s, and the gates must read byte-identical manifests to the ones kubeconform sees.
- **Reach a network, except in `pull`.**

---

## 5. Renderings become artefacts of the spec

Moving the renderers into one binary is only half an answer, because `rust/` keeps its own for the
runtime audience (§2). Two renderers is the state this whole plan exists to leave.

So the corpus grows a rendered half:

```
spec/v1/conformance/<case>/
  contract.json          what a producer emits            (exists)
  README.md              what the case is for             (exists)
  rendered/
    markdown.md          --format markdown                (new)
    markdown-loader.md   --format markdown-loader         (new)
    markdown-keys.md     --format markdown-keys           (new)
    config.toml          --format toml                    (new)
    schema.json          --format json-schema             (new)
    labels.txt           --format labels                  (new)
    Dockerfile.part      --format dockerfile              (new)
```

The binary is the reference renderer and the goldens are regenerated by it, under the same
`TERRACE_SPEC_BLESS` convention `spec/` already uses. Anything else that renders is checked against
those bytes: `rust/`'s library in `rust/tests/`, and Java's — if it ever grows a runtime audience of
its own — in its own suite. One command regenerates, two suites verify, and a drift is a failing
test in the implementation that drifted rather than a difference nobody compares.

This is worth more than it costs, and the reason is a property nothing has today. `CONFORMANCE.md`
says byte-identity is "reachable only between implementations sharing a loader *and* a type
mapping. Do not expect to reach it from another language." That is true of the *document*, and it
stays true. It was never true of the renderings — they are downstream of the document — and routing
them through one renderer means a Java service's README table and a Rust service's are the same
bytes, which no amount of careful reimplementation would have achieved.

---

## 6. The inventory

### From `rust/src/schema/cli/` — moves, and the feature is frozen

| Source | Lines | Lands as |
|---|---|---|
| `cli/format.rs` | 152 | `render`'s `--format` vocabulary |
| `cli/request.rs` | 279 | `clap` arguments, `stamp` |
| `cli/mod.rs` | 276 | dispatch, dissolved into `bin/main.rs` |
| `cli/verify.rs` | 137 | `image` |
| `markdown.rs`, `toml_example.rs`, `json_schema.rs`, `tree.rs` | 1,656 | `render/*` — **copied, not moved** (§5) |

`schema-cli` is not deleted. `Portfolio`'s Dockerfile runs
`cargo run … --example config-schema -- --format contract` today and must keep working through the
migration. What changes is that the feature **stops growing**: a new format lands in the binary
only, and phase 6 makes the example a thin `--format contract` emitter once every consumer has
moved.

### From `java/` — deleted rather than finished

| Source | Lines | Why |
|---|---|---|
| `schema/MarkdownRenderer.java` | 109 | `render --format markdown` |
| `schema/TomlExampleRenderer.java` | 381 | `render --format toml` |
| `schema/JsonSchemaRenderer.java` | 212 | `render --format json-schema` |
| `schema/Column`, `Node`, `Docs`, `Defaults`, `JsonSchemaOptions`, `TomlExampleOptions` | ~490 | support for the above |
| `refusal/ContractValidator.java` + 8 exceptions | ~155 + | `conform` is normative; a fast path may stay (§6.1) |
| `terrace-config-spec-tck` | ~200 | becomes a JUnit wrapper around `conform` |

Plus the five renderings `java/README.md` lists as open and which are now never written:
`markdown-loader`, `markdown-keys`, `contract`, `labels`, `dockerfile`. On the Rust line counts
those are another ~700 lines of Java that does not get written, and every one of them would have
had to be byte-identical to Rust's to be useful.

**This is the time-critical part of the plan.** Every week it waits, more of it is written.

### 6.1 The refusals are the one honest duplication

`FORMAT.md` says "A producer MUST fail rather than emit a document containing any of these". A
producer that emits and then has a separate tool refuse the result has not quite done that, and
the per-language check has better error messages besides: it knows the field, the annotation and
the source position, where the binary knows only a JSON pointer.

So both stay, with the roles named: **`conform` is normative** and runs in every build as the gate;
the per-language validator is a fast path whose only job is a better message. Where they disagree,
`conform` is right and the other is a bug. `java/`'s `ContractValidator` therefore survives §6's
deletion list — it is the one thing in that column with a reason.

### From `helm-charts/.github/scripts/` — the chart half

Unchanged from the first draft, and still the bulk of the work: ~15.0k lines of implementation
across 23 scripts, ~8.1k of tests, and an 825-line `just` module.

| Group | Files | Lines | Lands as |
|---|---|---|---|
| pure format | `config_contract`, `config_diff`, half of `config_shapes`, half of `config_testgen`, half of `config_secrets`, `config_paths` | ~4.7k | `document`, `union`, `classify`, `value`, `diff`, `shape`, `probe`, `secrets` |
| Kubernetes | `config_manifests`, `config_gate_document`, `config_gate_container`, `config_report` | 869 | `k8s`, `gate`, `report` |
| Helm | `config_declaration`, `config_bindings`, half of `config_shapes`, `config_readme`, `config_coverage`, `config_scaffold`, half of `config_testgen`, half of `config_secrets` | ~5.1k | `helm::*` |
| entry points | 8 scripts | 5.1k | `clap` and `report` |

Not moving, because none of it reads a contract: `add-tunable.py`, `audit-observability.py`,
`chart-index.py`, `check-chassis.py`, `check-immutable-fields.py`, `check-values-docs.py`,
`crd-schema-refs.py`, `extract-prometheus-rules.py`, `kube-schema-refs.py`,
`kube-version-floor.py`, `rule_anchors.py`, `schema-presets.py`, and `new-chart.py`'s chassis half.

---

## 7. Reading documents this repository did not write

Three rules the Python has wrong, invisibly, because it was written when there was one producer.
`java/terrace-config-spring-boot` makes all three real. The port fixes them; it does not carry them.

### 7.1 The reads belong to the loader, and the consumer must ask

`FORMAT.md`: the reads it tabulates are normative for `producer.loader == "figment"` "and for
nothing else. A consumer meeting a loader it does not know MUST skip step 2 and say so."

`config_gate_container.py:189-192` calls `check_text` then `check_parsed` unconditionally, and
`config_contract.py` never reads `producer` at all — the field does not appear in the module.
Against a `spring-boot` document that applies figment's measured patterns to text Spring's `Binder`
reads differently, and it fails in the expensive direction: a pattern refusing text the loader
accepts stops a deployment that was correct.

So `value` is a registry, not a pair of functions.

```rust
/// One loader's environment reads, as measured against that loader.
pub trait Reads {
    fn parse(&self, form: TextForm, text: &str) -> Option<Json>;
}

/// `None` means: skip step 2, and report it as skipped rather than passed.
pub fn reads_for(loader: &str) -> Option<&'static dyn Reads>;
```

Three rows at the start: `figment`, `terrace-java`, `spring-boot` — the three `producer.loader`
values `java/README.md` already names. An unknown loader yields a finding at warning severity, for
which `report`'s existing distinction between "found wrong" and "could not check" is exactly right.

**Each implementation proves its own row.** The table is data in `cli/`, and `rust/` and `java/`
each carry a test asserting their loader agrees with it — `rust/tests/contract_read.rs` is already
that test, against the prose. This is the dependency direction that makes the registry honest: the
binary states the reads, and the implementations are held to them, rather than the binary guessing
at three loaders it cannot run.

### 7.2 Tier 2 is an assumption, and one rule already makes it silently

`Union.structured_parent` decides that a name extending a `structured` key's spelling by
`dialect.nesting_separator` addresses a leaf of that map. True of figment and of `terrace-java`.
Not true of `spring-boot`, whose relaxed binding is exactly the case `CONFORMANCE.md` predicts:
"An implementation that hands naming to a binder with its own relaxed-binding rules will not reach
tier 2 without overriding it, and should not pretend to."

The rule, stated so it can be checked rather than remembered: **derive nothing the document
states.** Everything reading `env`, `env_file`, `secrets_file` and `unreachable` as published is
tier 1 work and stays unconditional — which is most of the toolchain, and is why a tier 1
Spring producer is usable by the gates on the day it emits its first document. Mechanically:
`grep -r nesting_separator cli/src` must return hits only in `value`, `conform` and `diff`.

### 7.3 `ty` is not a type name

`FORMAT.md` says so, and `java/` will emit `java.time.Duration` where `rust/` emits `u64`. The
Python is nearly clean by accident — two uses, a display set and a severity row, neither switching
behaviour on the value. Neither may start. `shape` and `probe` read `constraint`, `text_form` and
`text_constraint`, and the moment one `match` arm spells a Rust type name the binary is
single-producer again.

### 7.4 `schema_version: 1` is not hypothetical

A new producer implements the simple thing first, and the one non-Rust document in the corpus today
— `helm-charts`' `foreign-dialect.json` — is `schema_version: 1`, before `constraint` learned to
nest. `shape` must degrade to "no element schema published" rather than assume `items`, and the
generators must narrow rather than refuse.

That fixture is also **not valid against `spec/v1/contract.schema.json`**: it carries no `producer`
block and the schema requires one. The Python never noticed, because it never reads the field.
Correcting it is part of phase 1, and a hand-written fixture that drifted from the published schema
is itself the argument for `validate`.

### 7.5 The corpus grows on the consumer side

`spec/v1/conformance/` holds three producer cases. It must also hold the documents a *consumer* has
to survive, and none exist:

| Case | Asserts |
|---|---|
| `foreign-loader/` | step 2 skipped and reported, not silently performed |
| `schema-version-1/` | `shape` degrades and the generators narrow, rather than failing |
| `tier-1-spellings/` | no rule re-derives a spelling the document states |
| `foreign-ty/` | a type vocabulary from another language changes no behaviour |

Cheap to write, checkable before any Java image exists, and the tests that catch the Java
integration breaking months before there is a contract to vendor.

---

## 8. Helm vocabulary

The chart declaration (`config-contract.yaml`), the `@config` marker language and the round-trip
enrolment file are a chart repository's conventions, arriving in a repository that publishes a
language-neutral format. Two things keep that from being a mistake.

**They do not enter `spec/v1/`.** That directory is the envelope and its `$id` is a promise:
"`spec/v1/` describes `terrace_contract: 1` and will not be edited to describe anything else." They
land in an independently versioned tree:

```
spec/helm/v1/
  DECLARATION.md           what config-contract.yaml means
  declaration.schema.json  validates one, and ships with the binary
  MARKERS.md               the @config grammar, the five refusals, where a marker may sit
  ENROLMENT.md             the round-trip test enrolment
```

`terrace-contract validate --declaration charts/x/config-contract.yaml` then checks a chart's own
file against a published schema — a gate `check-config.py` cannot offer, because that shape lives
only in `config_declaration.py`'s parser.

**They sit behind the `helm` feature**, above `k8s`, above the core. `--no-default-features` in CI
is the mechanical check that no Helm concept reached `document`, `render` or `classify` — the half
a language's build links.

---

## 9. Distribution

Four channels, and for the first time the container image matters most.

### 9.1 Container image — for Docker builds

`ghcr.io/timschoenle/terrace-contract:<version>`, multi-arch, signed. This is what makes the build
integration language-agnostic, because a build stage needs no toolchain, no download step and no
platform detection:

```dockerfile
FROM ghcr.io/timschoenle/terrace-contract:1 AS contract
COPY --from=contract-builder /out/contract.json /in/contract.json
RUN terrace-contract conform --tier 2 /in/contract.json \
 && terrace-contract render --format labels /in/contract.json > /out/labels
```

Today `Portfolio`'s Dockerfile does the equivalent with
`cargo run -p portfolio-config --features config-schema --example config-schema -- --format contract`
in a `contract-builder` stage, a committed `LABEL` block between
`# terrace-config:labels:begin`/`:end` markers, and a `verify-labels` example run against
`docker inspect`. After this, only the first of those stays Rust; the block regeneration, the diff
and the read-back are `terrace-contract render --format dockerfile` and
`terrace-contract image verify`, and a Spring Boot service's Dockerfile is the same file with a
different `contract-builder` stage.

### 9.2 Release binaries

Cross-compiled and attached to the release, `SHA256SUMS` beside them, cosign keyless. `helm-charts`
pins a version in its `justfile` beside `jv`, `oras` and `cosign` — the pattern it already chose,
for the stated reason that a pinned single binary "runs identically in a Git Bash shell and keeps a
`pip install` out of a recipe" — and this retires two of the three.

| Target | Why |
|---|---|
| `x86_64-unknown-linux-musl` | runners; static, no glibc floor |
| `aarch64-unknown-linux-musl` | ARM runners, and the image's second arch |
| `x86_64-pc-windows-msvc` | the development shell |
| `aarch64-apple-darwin` | local |

Signing matters more than usual here: `helm-charts` already refuses a *contract* whose signer
identity does not match `contract_signer`, and a tool that validates signed documents arriving
unsigned is a hole in the same argument.

### 9.3 Composite action

`TimSchoenle/actions/actions/terrace-contract/install` — version in, checksum and signature
verified, cached, on `PATH`. Every job that installs `jv` by release URL becomes one `uses:`, and
Renovate already tracks that repository's tags.

### 9.4 Gradle plugin, and the Helm plugin

A Gradle plugin is now worth building rather than speculative: it wires the annotation processor's
descriptor to a `contractJson` task, then invokes the binary for `conform` and the renderings, so a
Java service's build has the same three lines a Rust one does. A Helm plugin (`helm terrace-contract
explain …`) stays the thin third channel it was — the same binary under a different name, for
operators without a checkout.

### 9.5 The registry, by delegation

`pull` shells out to `oras` and `cosign` rather than linking an OCI client. The signature *policy* —
which workflow identity a contract must be signed by — belongs to the consuming organisation, not
to this binary; `helm-charts` already pins both tools; and an OCI client plus a TLS stack is a large
dependency and a large attack surface for one subcommand. Behind a trait, so a future
`--features oci-native` supplies the other implementation without moving the rules.

---

## 10. Versioning

`cli/` is a third release-please package, with `include-component-in-tag: true`, tagging
`terrace-contract-v1.2.0` — the shape `java` already uses on that branch, and the shape `rust`
deliberately does not.

**It versions independently, and the first draft of this plan was wrong to say otherwise.** That
draft argued for lockstep with `terrace-config` on the grounds that one number for format, producer
and consumer makes "gated by v0.12.0" a statement about the format. With three implementations that
inverts: a Spring Boot shop pinning a version that is also the Rust crate's is pinning something it
does not use, and every Rust release would move a number the Java side has to justify.

What replaces the lockstep is a compatibility statement the binary can actually keep: it declares
the **envelope** versions it reads — `terrace_contract: 1`, `schema.schema_version: ≤ 2` — and gates
on those, never on `producer.name` or `producer.version`. `terrace-contract --version` prints both,
and the README carries the matrix. That is the number an implementation cares about, and it is the
one `spec/` already versions.

---

## 11. Migration

Seven phases. The order changed from the first draft: **the producer side goes first now**, because
`java/` is writing renderers this month and the chart half is not urgent in the same way.

**Phase 0 — the parity harness.** Before anything. Two directions: `cli`'s renderers against
`rust/`'s over the corpus (byte equality, which is §5's goldens arriving early), and later the
Python against `cli` over the whole `helm-charts` tree. Every phase's exit criterion is "parity, or
a documented deliberate difference".

**Phase 1 — the core and the renderings.** `document`, `render/*`, `stamp`, `conform`, `validate`,
`image`. Ship `render`, `stamp`, `conform`, `validate`, `image verify`. Land §5's `rendered/`
goldens and §7.5's four consumer cases, and correct `foreign-dialect.json`. Land the loader
registry from §7.1 — it is a constraint on the reader's shape, not a feature on top of it, and
retrofitting one to a call site that assumed figment is how the Python arrived where it is.
*Deletes nothing yet, and unblocks everything.*

**Phase 2 — the Java cutover.** `java/` deletes its renderers and its options classes, keeps
`ContractValidator` as the fast path (§6.1), and `terrace-config-spec-tck` becomes a JUnit wrapper
around `conform`. The Gradle plugin lands. A Spring Boot service builds an image with a contract,
labels and a Dockerfile block that are byte-identical to a Rust service's. **This is the phase the
whole plan is for**, and everything after it is the chart half.

**Phase 3 — the gates.** `k8s`, `gate::*`, and the `helm::declaration` reader they need. Ship
`check`. Delete `check-config.py`, `config_gate_*.py`, `config_manifests.py`, `config_contract.py`,
`config_report.py` and the `jv` pin from `helm-charts`.

**Phase 4 — the marker language.** The YAML spike, then `helm::bindings`, `helm::schema_block`,
`helm::coverage`. Ship `bindings`, `shapes`, `coverage`. Delete four more scripts. Highest risk,
and the reason the spike is first.

**Phase 5 — the derived documents.** `diff`, `secrets`, `helm::readme`, `probe`, `helm::unittest`.
Ship `diff`, `secrets`, `readme`, `tests`, `explain`. Delete eight scripts.

**Phase 6 — the writers, the network, and the cleanup.** `helm::scaffold`, the adopt path, `oci`,
`pull`, `sync`. Delete the rest, and `lint-python` if nothing else needs it. `rust/`'s
`schema-cli` example becomes a `--format contract` emitter. `just/contracts.just` drops from 825
lines to a header and wrappers — keep the header, which is the best statement of why any of this
exists.

The writers are late because a generator that is wrong writes its mistake into the tree, where the
parity harness cannot see it as a diff; the network is last because it is the one failure that is
not the tool's fault.

---

## 12. Testing

**The renderings are tested by the corpus** (§5), which is the strongest form available: one set of
bytes, regenerated by one command, verified by every implementation that renders.

**The chart rules port as integration tests.** `helm-charts`' 8,093 lines are already written as
"construct a document and a manifest, call the rule, read the findings back", which is an
integration test in Rust's vocabulary and keeps the public surface honest. Its fixture corpus moves
to `cli/tests/fixtures/`, with `foreign-dialect` and `remote-ref` promoted into
`spec/v1/conformance/` as the refusal cases it lacks.

**The parity harness stays** after the migration, pointed at `helm-charts` `main`, as the only test
with a real corpus.

**`cli/fuzz/` gains two targets**: the document reader over arbitrary JSON, because it now parses
documents written by producers this repository does not control, and the marker scanner over
arbitrary YAML, because it is line-keyed and line-keyed parsers are where panics live. The oracles
run under `cargo test` without libFuzzer, as `rust/fuzz/` already does.

**`rust/tests/contract_read.rs` gets a third side.** Today it implements the documented read and
asserts the loader agrees. Once `value` exists it becomes prose, loader, and the shipped registry —
and `java/` grows the same test against its own two rows. That is what stops this exercise from
re-creating the drift it exists to remove.

---

## 13. What it costs each side

**`rust/`** — nothing at runtime. `schema-cli` freezes and is eventually thinned. Its renderers
gain a golden test against the corpus. The dependency arrow is `cli/` → nothing, enforced by a
`cargo tree` check.

**`java/`** — deletes ~1,200 lines and never writes ~700 more. Gains a Gradle plugin and a
binary in its build. `ContractValidator` stays as a fast path; the TCK becomes a wrapper.

**`spec/`** — grows `rendered/` goldens per case, four consumer cases, and a `helm/v1/` tree. The
`TERRACE_SPEC_BLESS` convention covers the new bytes unchanged.

**`helm-charts`** — loses ~23,000 lines, `python3` + PyYAML, and the `jv` pin. Keeps `helm`,
`kubeconform`, `kube-linter`, `helm unittest` and `just render`.

**CI here** — a new workspace in the `clippy`, `features`, `test`, `doc` and `deny` matrices
including a `--no-default-features` row; a `release-binaries` workflow; a container build; and a
`parity` workflow that checks out `helm-charts` at a pinned ref.

---

## 14. Risks

| Risk | Answer |
|---|---|
| **`java/` finishes its renderers first** | The one urgent item in this plan. Phase 1 is scoped to make phase 2 possible in weeks, and §6's deletion list should be agreed before more of it is written |
| #98 has not merged | Nothing here depends on it landing first; phase 0 and phase 1 run against either tree, and only the paths in §3 change |
| Two renderers in Rust | §5's goldens, from phase 1. Until they exist, the parity harness is the check |
| A rule quietly assumes figment again | `grep -r nesting_separator cli/src` in CI, and `reads_for` returning `Option` rather than a default — the type makes the skip unforgettable |
| `cli/` grows a dependency on `rust/` | A `cargo tree` assertion in CI, because it is the one claim in §3 that nothing else would notice breaking |
| Comment-preserving YAML has no good crate | Spike before phase 4; the fallback is the Python's own approach, which is known to work and is not elegant |
| The container image is a new supply-chain artefact | Signed and attested like the binaries, from the same workflow, or it is a weaker link than the thing it validates |
| A chart-side fix needs a release here | `TERRACE_CONTRACT_BIN` override in the recipes, which is also what makes the parity harness possible |
| Scope | ~24,000 lines to port. The phases are independently shippable; phases 0–2 are perhaps a fifth of it and deliver the multi-language payoff on their own |

---

## 15. Open questions

1. **The YAML pair.** §3.1. Blocks phase 4, nothing earlier.
2. **Does `render` accept a bare schema document, or only a contract?** A contract requires an
   `App`, and a service rendering a README table has no build identity to state. Accepting both is
   two input shapes; requiring a contract is a synthetic `App`. Leaning: accept both, since
   `--format json`'s own output is the bare schema and refusing to re-read it would be odd.
3. **Where does each binder's read table live?** `FORMAT.md` under a heading of its own is what
   `CONFORMANCE.md` mandates, and §7.1 makes `cli/` the executable copy. Confirm the Java side
   agrees before `spring-boot` emits a document, because the table is what makes its
   `text_constraint` fields actionable rather than decorative.
4. **`spec/helm/v1/` here, or in `helm-charts`?** Here, because the binary validates against it;
   there, because it describes that repository's conventions and no format. Written as "here".
5. **Does `conform` need to run inside the producer's build, or beside it?** §6.1 says beside, with
   a per-language fast path. A Gradle plugin makes "inside" nearly free for Java; `build.rs` does
   not, for Rust.
6. **Does the parity harness check out `helm-charts`, or vendor a fixture tree?** Checked out finds
   more; vendored is reproducible. Start checked out at a pinned ref.
