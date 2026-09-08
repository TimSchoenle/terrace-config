# Migrating to a multi-language repository

Moving the Rust crate under `rust/`, and adding two Java producers — vanilla and Spring Boot — that
conform to `spec/v1/`.

**Status: plan, not yet started.** Nothing in this document has been implemented. It is written to
be picked up by someone — or some agent instance — with no memory of the session that produced it,
so it states the decisions already taken and the reasoning behind them rather than assuming either.

**Every relative link below is written against the post-move layout of section 2.** Links into
`rust/` therefore do not resolve until Phase 0 has landed; links into `spec/` and `docs/` resolve
either way, because neither directory moves.

---

## 0. Where this starts

[#94](https://github.com/TimSchoenle/terrace-config/pull/94) published the contract format as
[`spec/`](../spec/): a directory-versioned, language-neutral description of the document, with a
meta-schema, three reference documents, and three conformance tiers. It was written for exactly
this migration — [`CONFORMANCE.md`](../spec/v1/CONFORMANCE.md) already contains an *Adding an
implementation* procedure and a set of open questions phrased against Spring's binder.

So the format work is done and the repository work is not. Today the repository is a Rust crate
that happens to contain a spec; it needs to become a spec that happens to have three
implementations, one of which is Rust.

**Read before starting:** [`spec/README.md`](../spec/README.md),
[`spec/v1/CONFORMANCE.md`](../spec/v1/CONFORMANCE.md) in full, and
[`spec/v1/FORMAT.md`](../spec/v1/FORMAT.md) at least as far as *Reading a variable*. The rest of
this document assumes their vocabulary — tier, producer, loader, dialect, `text_form`,
`unreachable` — without redefining it.

---

## 1. Decisions taken

Each of these was a fork in the road. They are settled; changing one invalidates a phase below, so
change them deliberately rather than by drift.

| # | Decision | Why, and what it rules out |
|---|---|---|
| 1 | **Monorepo. Rust moves to `rust/`, Java lands in `java/`, `spec/` stays at the root.** | `spec/` is the shared artefact and belongs above both implementations, not inside one. `CONFORMANCE.md` requires that a re-blessed corpus diff ship "in the pull request that caused it rather than in a follow-up" — only one repository can honour that. Rules out a separate `terrace-config-java` repository, where a spec change and its two conformance updates could never be one commit. Costs a large mechanical move (Phase 0) and permanent asymmetry if skipped. |
| 2 | **The vanilla implementation is a Java port of the five layers**, not a producer bolted onto somebody else's binder. `producer.loader` is `terrace-java`. | A producer must name the library whose environment reads its `text_constraint` patterns were measured against. Wrapping Jackson would mean publishing Jackson's reads and shipping no loader; Java would get a document generator and no runtime. Porting the layers is the only option where `terrace-java` is a thing a service can actually load configuration with, and the only one that can honestly aim at tier 2. |
| 3 | **Schema declaration is an annotation processor**, mirroring `#[derive(Describe)]`. | Java reflection can see types, generics and enum constants; it cannot see the Javadoc, and it cannot refuse to compile. [`rust/docs/SCHEMA.md`](../rust/docs/SCHEMA.md) treats *"a field whose type publishes no shape is a compile error"* as the whole point of the feature — a runtime-reflection design turns that into a generation-time failure discovered by whoever runs the generator, which is later and quieter. Rules out `@Description("…")` duplicated beside the Javadoc. |
| 4 | **Both producers are built in parallel over a shared core**, rather than one then the other. | Forces the core/adapter boundary to be correct rather than refactored into existence, and the two loaders disagree about enough — naming, list spellings, indirection — that a core designed against one of them alone would encode its assumptions. |
| 5 | **Maven, Java 21, `dev.terrace` groupId.** | Not asked, chosen: Maven is Spring Boot's documented default and the reproducibility story is simpler; Java 21 is the current LTS and above Spring Boot 3's floor; `dev.terrace` matches the OCI label namespace `dev.terrace.config.*` that `FORMAT.md` already fixes. Revisit freely before Phase 1 lands; after it, the coordinates are published. |

### Tier targets

State these in each module's README, and do not claim them before the corpus passes.

| Implementation | `producer.name` | `producer.loader` | Target | Why not higher |
|---|---|---|---|---|
| Rust | `terrace-config` | `figment` | tier 3 against itself | — |
| Vanilla Java | `terrace-config-java` | `terrace-java` | **tier 2** | Tier 3 needs a shared loader *and* a shared type vocabulary; `u16` has no Java spelling. `CONFORMANCE.md` says not to treat that as a defect. |
| Spring Boot | `terrace-config-spring` | `spring-boot` | **tier 1**, divergence documented | Spring's relaxed binding uses `_` where terrace uses `__`, and derives names by a rule terrace does not share. Reaching tier 2 would mean overriding Spring's own naming, which is the thing a Spring user came for. `CONFORMANCE.md`: *"Say tier 1 and document the mapping."* |

---

## 2. Target layout

```
.
├── spec/                     unchanged, at the root, the shared artefact
│   ├── README.md
│   └── v1/
│       ├── FORMAT.md         gains one reads section per new loader
│       ├── CONFORMANCE.md
│       ├── contract.schema.json
│       └── conformance/      gains a per-language source block per case
├── docs/                     repo-level docs only
│   └── multi-language-plan.md    ← this file, stays here
├── rust/                     everything the crate is today
│   ├── Cargo.toml
│   ├── CHANGELOG.md
│   ├── README.md
│   ├── deny.toml
│   ├── docs/                 SCHEMA.md, CONTRACT.md, RELOAD.md, … and config-contract-plan.md
│   ├── src/  macros/  tests/  examples/  fuzz/
├── java/
│   ├── pom.xml                                aggregator, no code
│   ├── terrace-config-annotations/            @TerraceConfig and friends; compile scope only
│   ├── terrace-config-core/                   document model, renderers, the eight refusals
│   ├── terrace-config-processor/              JSR-269 processor → descriptors
│   ├── terrace-config-loader/                 the vanilla five-layer loader
│   ├── terrace-config-spring-boot/            starter + producer over Spring's Binder
│   └── terrace-config-spec-tck/               corpus runner and tier comparators
├── README.md                 front door for the repository, not for the crate
├── LICENSE  SECURITY.md
└── .github/
```

Three things in that tree are load-bearing and easy to get wrong:

- **`spec/` does not move.** `.gitattributes` pins `spec/** text eol=lf` because the corpus is
  compared byte for byte and the tree is authored on Windows. Moving it would silently invalidate
  every stored document on the next checkout.
- **`docs/` splits.** Everything currently under `docs/` describes the Rust crate's features and
  moves with it to `rust/docs/` — `config-contract-plan.md` included, since it is the design that
  produced the Rust implementation. Root `docs/` holds repo-level documents; this file is the
  first.
- **`terrace-config-annotations` is separate from `-core`.** A service annotating its
  configuration types needs the annotations on its compile classpath and nothing else; the model
  and its JSON belong to whatever renders the contract, which for most services is a build step,
  not the application.

---

## Phase 0 — make room

Two pull requests. Neither changes a line of behaviour, and the whole risk of the migration is
concentrated here: every path in CI, in the release automation, in the README template, and in one
test is currently written against a repository root that is about to stop being the crate root.

### PR 1 — move the crate under `rust/`

**Before anything else, resolve the blocker.** CI delegates to composite actions in the
maintainer's own repository:

```
TimSchoenle/actions/actions/rust/clippy
TimSchoenle/actions/actions/rust/test
TimSchoenle/actions/actions/rust/cargo-check
TimSchoenle/actions/common/readme-variables
```

None of them is invoked with a working directory today because none has needed one. Check whether
each accepts a `working-directory` (or manifest-path) input. If not, the choices are: add the input
upstream first — it is the same maintainer's repository, so this is a prerequisite PR, not a
negotiation — or inline plain `cargo` steps in `ci.yml` and lose the shared hardening. **Prefer
adding the input upstream.** Do not start the move until this is answered; a half-moved tree with a
red required check is the worst place to discover it.

**The move.**

```bash
mkdir rust
git mv src macros tests examples fuzz docs Cargo.toml deny.toml CHANGELOG.md rust/
git mv README.md rust/README.md
```

`spec/`, `LICENSE`, `SECURITY.md`, `.github/`, `.gitignore`, `.gitattributes`, `renovate.json` and
the release-please files stay at the root. A new root `README.md` is written in PR 2; between the
two PRs the repository has none, which is acceptable inside one merge train and not across a
release.

**Then the edits, all of which are mechanical and none of which is optional.**

| File | Change |
|---|---|
| `rust/tests/spec.rs` | `spec_dir()` builds `CARGO_MANIFEST_DIR/spec/v1`. The manifest directory is now `rust/`, so it must climb one level. **This is the only source change in the move**, and if it is missed the spec suite fails loudly, which is the good outcome. |
| `rust/Cargo.toml` | `readme = "README.md"` now resolves to `rust/README.md` — correct, no edit. `[workspace] members = ["macros"]` — correct, no edit. |
| `rust/docs/CONTRACT.md` | Three links to `../spec/v1/FORMAT.md` become `../../spec/v1/FORMAT.md` (lines 57, 221, 300). |
| `spec/README.md` | `../docs/CONTRACT.md` → `../rust/docs/CONTRACT.md` (line 57). |
| `spec/v1/FORMAT.md` | Link definition `[docs/CONTRACT.md]: ../../docs/CONTRACT.md` → `../../rust/docs/CONTRACT.md` (line 22). |
| `spec/v1/CONFORMANCE.md` | `../../tests/spec.rs` → `../../rust/tests/spec.rs` (line 75), and the `cargo test` invocations under *Running it* gain `--manifest-path rust/Cargo.toml` or a `cd rust`. |
| `.github/workflows/ci.yml` | Every Rust job runs in `rust/`. The `msrv` job greps `rust/Cargo.toml`. The `deny` job passes `manifest-path: rust/Cargo.toml` (and `deny.toml` sits beside it). The `fuzz` job's `working-directory: fuzz` becomes `rust/fuzz`; its *Short campaign* step runs from `rust/` and keeps its internal `fuzz/corpus/…` paths unchanged. |
| `.github/workflows/update-files.yaml` | Lockfile generation runs against `rust/Cargo.toml` and `rust/fuzz/Cargo.toml`. |
| `.github/workflows/docs.yml` | `bash .github/scripts/readme-variables.sh` gains the argument `rust/Cargo.toml`; the `readme-variables` action needs its manifest and docs-directory inputs pointed at `rust/`. |
| `.github/scripts/readme-variables.sh` | Already parameterised — `manifest="${1:-Cargo.toml}"`. Change the default to `rust/Cargo.toml` so a local run still works with no argument. |
| `.github/templates/README.md.hbs` | Becomes the *crate* template rendering to `rust/README.md`. MSRV badge link `(Cargo.toml)` → `(../rust/Cargo.toml)` as rendered; `fuzz/README.md` → `fuzz/README.md` relative to `rust/` (unchanged); `spec/` → `../spec/`; `LICENSE`, `SECURITY.md` → `../LICENSE`, `../SECURITY.md`. |
| `release-please-config.json`, `.release-please-manifest.json` | The package key `"."` becomes `"rust"` in both files, and the manifest keeps the version: `{"rust": "0.11.0"}`. Set `include-component-in-tag: false` **on the package** so released tags stay `v0.12.0` and every existing consumer pin keeps resolving. See PR 2 for the multi-package shape. |
| `renovate.json` | The `fuzz/Cargo.toml` file-name rule becomes `rust/fuzz/Cargo.toml`. |

**Verification, in order.** Do not proceed past a red step.

```bash
cd rust && cargo fmt --all --check && cargo clippy --workspace --all-features --all-targets
```

```bash
cd rust && cargo test --workspace --all-features
```

```bash
cd rust && cargo test --all-features --test spec
```

The third is the one that proves the move: it reads `spec/v1/` from a manifest directory that is no
longer the repository root.

**The consumer smoke test — do not skip this.** The crate is distributed as a git dependency with
no `path` component:

```toml
terrace-config = { git = "https://github.com/TimSchoenle/terrace-config", tag = "v0.11.0" }
```

Cargo resolves a git dependency by searching the repository for a package of that name, so a
package in a subdirectory is expected to work — but "expected to work" is not a thing to discover
after tagging a release. Add a CI job that builds a throwaway crate depending on the checkout by
git URL and fails if resolution does not find `terrace-config` under `rust/`. Keep the job
permanently; it is the only check that the published install snippet is true.

### PR 2 — multi-language repository wiring

- **A root `README.md`** and a template for it under `.github/templates/`. It is a front door, not
  a second copy of the crate README: what the project is, the three implementations and their
  tiers in one table, where the spec lives, and one link each into `rust/README.md`,
  `java/README.md` and `spec/README.md`.
- **release-please becomes multi-package.** `rust` keeps `include-component-in-tag: false` and its
  bare `vX.Y.Z` tags. Each Java artefact that is released independently gets a package entry with
  `component` set and `include-component-in-tag: true`, so its tags are `java-v0.1.0` and cannot
  collide with the crate's. Verify against release-please's own schema that
  `include-component-in-tag` is honoured per-package in the version installed; if it is not, the
  fallback is to release the Java modules as one component and accept a single Java version line.
- **CI gains Java jobs and path filters.** The aggregate `ci` job is already written as
  `if: always()` with a check that passes on skipped jobs, so filtering by path is safe: a
  Java-only pull request can skip the 25-minute fuzz job without the required check disappearing.
  Add `rust` and `java` path filters, and add every new Java job to the `needs:` list of `ci` — a
  job absent from that list is a job branch protection does not enforce.
- **Renovate learns Maven.** Enable the maven manager for `java/**/pom.xml`, and group the Java
  test-scoped dependencies the way `fuzz/` is grouped today.
- **`docs/CONTRIBUTING.md`**, or a section in the root README: the two toolchains, the two test
  commands, and the rule that a spec change and both implementations' corpus updates are one pull
  request.

---

## Phase 1 — the Java skeleton and the TCK

### PR 3 — `java/` build and `terrace-config-spec-tck`

The TCK comes first, before any producer, and it is testable before any producer exists — it can
run against the stored corpus itself. That is the point: a harness that has never failed is a
harness nobody has debugged.

**What it contains.**

- A **meta-schema validator**. Use `com.networknt:json-schema-validator`, which handles draft-07
  and resolves nothing over the network. `contract.schema.json` is self-contained by design; the
  validator must be configured to refuse remote references outright rather than to happen not to
  need them.
- **Two entry points**, matching the Rust suite: the whole envelope against the schema root, and
  the `schema` half alone against `#/$defs/schema`, because that half is published as an artefact
  in its own right.
- **A tier comparator.** Given a produced document and a stored case, compare only what the claimed
  tier covers:
  - *tier 1* — schema-valid, the eight refusals honoured, a `producer` block present, and the
    document byte-stable across two renders of the same input;
  - *tier 2* — tier 1, plus field-for-field equality of `env`, `env_file`, `secrets_file`,
    `env_aliases`, `env_file_aliases`, `secrets_file_aliases` and `unreachable` for every key;
  - *tier 3* — byte equality after substituting `producer.version` with `0.0.0-conformance`.
  The comparator's failure output must be a readable diff, not an assertion message. The diff *is*
  the deliverable when a rendering changes.
- **The `producer.version` substitution**, `0.0.0-conformance`, exactly as `rust/tests/spec.rs`
  applies it. Nothing else is normalised — not field order, not escaping, not whitespace.

**How it is proved before a producer exists.** Two tests that need no implementation: every stored
`contract.json` validates against the meta-schema, and each case round-trips through the core model
once that lands (PR 4). A corpus case that no longer validates is the one failure a corpus alone
cannot catch — an expectation blessed while wrong — and the Rust suite checks for it, so the Java
side must too.

**Also in this PR:** the aggregator `pom.xml`, the module skeletons, the formatter and linter
configuration, and a `java/README.md` that says which modules exist and claims no tier yet.

---

## Phase 2 — the shared core and the processor

### PR 4 — `terrace-config-core`

The document model and everything that is true of a contract regardless of which loader produced
it. No Spring, no reflection over user types, no I/O beyond serialising.

- **The model**: envelope, `producer`, `app`, `schema` (`dialect`, `loader[]`, `keys[]`),
  `json_schema`, `external`. Required-ness follows the meta-schema exactly — the required lists are
  reproduced in section 8 below, and `dialect.indirection_suffix` being a required non-empty string
  is the one with consequences (see PR 7).
- **Determinism is a tier 1 requirement**, not a nicety. Byte-stable over one source tree means:
  explicit `@JsonPropertyOrder` on every model class, no `HashMap` anywhere in the render path, and
  a fixed pretty-printer. Add a test that renders the same input twice and compares bytes; add
  another that renders it in a JVM started with `-XX:hashCode=2` if you want the failure to be
  reproducible rather than occasional.
- **The eight refusals**, from [*What a producer MUST refuse*](../spec/v1/FORMAT.md#what-a-producer-must-refuse).
  Each gets its own exception type and its own test. `CONFORMANCE.md` is explicit that a producer
  emitting a secret with a default "is not tier 1 however well-formed the JSON is" — these are not
  validation niceties, they are the tier.
- **The `unreachable` machinery**: `unnameable` for a path that does not survive the case fold or
  already carries the nesting separator, `indirection` for a spelling colliding with another key's
  `_FILE` variable. This is shared between both producers and is most of what tier 2 costs.
- **The renderings**, mirroring the Rust `Format` enum: `json`, `markdown`, `markdown-loader`,
  `markdown-keys`, `toml`, `json-schema`, `contract`, `labels`, `dockerfile`. `contract` and
  `json-schema` are required for conformance; the markdown trio and `toml` are what makes the
  implementation usable and can land in a follow-up if the PR gets too large.

**Test it against the corpus without a producer**: deserialise each stored `contract.json` into the
model, re-serialise, and compare bytes. That single test exercises the whole model and every
ordering decision, and it fails the moment the model cannot represent something the format allows.

### PR 5 — `terrace-config-processor`

A JSR-269 annotation processor generating, for each annotated type, a descriptor class the
producers consume. It is the Java answer to `#[derive(Describe)]`, and the attribute vocabulary maps
across nearly one to one — [`rust/docs/SCHEMA.md`](../rust/docs/SCHEMA.md) is the specification for what each one
means, and divergence from it needs a reason written down.

| Rust | Java | Note |
|---|---|---|
| `#[derive(Describe)]` | `@TerraceConfig` | On a class, or on an enum, where it reports the constants as the values one key accepts. |
| doc comment | Javadoc | Read with `Elements.getDocComment()`. Reflection cannot see this; it is the reason the processor exists. |
| `#[config(nested)]` | `@Nested` | |
| `#[config(secret)]` | `@Secret` | |
| `#[config(values)]` / `values_from` / `values(…)` | `@Values`, `@Values(from = X.class)`, `@Values({"a","b"})` | |
| `#[config(range(…))]` | `@Range(min=, max=, exclusiveMin=, exclusiveMax=)` | |
| `#[config(element…)]` | `@Element`, `@ElementValues` | One level into a container. |
| `#[config(note = "…")]` | `@Note("…")` | |
| `#[config(skip)]` | `@Skip` | |
| `#[serde(alias)]`, `rename_all`, `default`, `deny_unknown_fields` | Jackson's `@JsonAlias`, `@JsonNaming`, `@JsonIgnoreProperties(ignoreUnknown = false)` | Read what the ecosystem already annotates, exactly as the Rust derive reads `serde`. Do not invent a second annotation for something Jackson already says. |

**The compile error is the feature.** A field whose type is a name the processor does not recognise
— not a leaf spelling, not a container, not something carrying `@TerraceConfig` — must fail the
compilation, with a message naming the field, its type, and every annotation that would resolve it.
Rust's version of that error is in `SCHEMA.md` under *A named type has to say something*; copy its
shape. A field that silently publishes nothing is the exact defect the whole feature exists to
close.

Test with `com.google.testing.compile:compile-testing`: one test per resolvable annotation, and one
per refusal asserting the diagnostic text.

---

## Phase 3 — the two producers, in parallel

### PR 6 — `terrace-config-loader`, the vanilla five layers

A Java port of what [`rust/src/`](../rust/src/) does, in the order the layers resolve: type
defaults, a TOML file or directory, prefixed environment variables, a directory of key-named files,
and `_FILE` indirection. The Kubernetes behaviour is not incidental and must port with it — follow
the `..data` symlink, skip dot-prefixed entries, and stat the *target* rather than the link, since
the Java equivalent of the Rust bug (`DirEntry::metadata()` not following symlinks) is
`Files.readAttributes` with `NOFOLLOW_LINKS`, and getting it wrong reports every real key as not a
file.

Shadowing between the last three layers fails the load by default, with a `LastWins` policy for a
codebase migrating onto it — same as the crate.

**Then measure the reads, and generate the table.** `CONFORMANCE.md` step 2 is emphatic: *measure,
do not derive*. Build a JUnit harness that feeds the probe set through the binder and records what
it accepts —

```
+5   007   1_000   0x1F   1e3   TRUE   true   yes   1   " 1 "   ""   a,b   ["a","b"]
```

— and renders the two normative tables (per layer, and per `text_form`) in the shape the figment
ones take in `FORMAT.md`. Write them into `FORMAT.md` between marker comments under a heading of
their own, regenerated by a bless flag mirroring `TERRACE_SPEC_BLESS`:

```bash
cd java && mvn -pl terrace-config-loader test -Dterrace.reads.bless=true
```

A hand-written table drifts from the binder silently, and every published pattern must be a
*superset* of what was measured — a pattern narrower than the loader stops a deployment that was
correct. Generating it from the measurement is what makes the normative claim falsifiable.

### PR 7 — `terrace-config-spring-boot`

**Read this first: the Spring module is a starter, not only a producer.** `dialect` requires
`prefix`, `nesting_separator` and `indirection_suffix`, all non-empty strings, and Spring has no
`_FILE` indirection at all. A producer describing stock Spring cannot emit a conformant dialect
without naming a suffix nothing implements — which is a lie the format has no way to catch and
every downstream gate would pass on.

So the module ships the missing layers as Spring machinery, and the producer describes what is
actually there:

- an `EnvironmentPostProcessor` adding `_FILE` indirection over the environment;
- the secrets directory via Spring's own `spring.config.import=configtree:/…`, which is that layer
  under another name and should be used rather than reimplemented.

Then the producer walks `@ConfigurationProperties` types — annotated with the same
`@TerraceConfig` vocabulary from PR 5 for everything Spring cannot say — and derives spellings from
Spring's relaxed binding, not from terrace's.

**Where it diverges, and why it is tier 1.** Spring joins path segments with `_`, terrace with
`__`. Spring's canonical form strips hyphens and case-folds before the join. Two different key
paths can therefore reach the same environment spelling under Spring where terrace keeps them
apart. That is a real difference in what deployments are valid, it is not a bug in either, and it
is precisely what tier 1 exists to allow. Document the mapping in `java/terrace-config-spring-boot/README.md`
and state tier 1 there.

Measure Spring's `Binder` with the same harness and the same probe set, and generate a second reads
section in `FORMAT.md` under `spring-boot`.

### PR 8 — fixtures, corpus, tier claims

For each of `minimal`, `full-surface` and `unnameable-key`: build the case in both Java
implementations, render it, and run it through the TCK at the tier that implementation claims. Then
add the Java source to each case's `README.md` under *Source*, beside the Rust — the case READMEs
already promise "the source that produces it in each language" and currently show only one.

Expect `full-surface` to be where the disagreements land. Its README says so and names two of them
in advance: `u64` for `ttl_secs` is an integer with a lower bound of zero and no expressible upper
bound, so a Java `long` publishes whatever bound it can *certainly* justify; `Option<String>` is
"may be absent", which is `required: false`, and the `ty` string is the producer's own and never
read by a consumer. Neither is a defect. Divergence beyond those two needs a written reason before
it is blessed.

Wire the TCK into CI as a required job on the aggregate `ci` check.

---

## Phase 4 — the questions a second implementation exists to answer

`CONFORMANCE.md` records three open questions rather than guessing at them, because inventing a
vocabulary from one data point is how a format acquires a field nobody can implement. Phase 3
produces the second data point. Answer them then, one PR each, and not before.

1. **How a `structured` key is spelled in one variable.** The hard half is not comma-separated
   values, it is that Spring also binds *indexed* names — `…_REPOS_0_`, `…_REPOS_1_` — which are
   variables the format has no key for at all. Step 4 of
   [*Reading a container*](../spec/v1/FORMAT.md#reading-a-container) rejects a prefixed variable
   matching no key, so a correct Spring deployment fails the gate today. The likely answer is a
   per-loader statement about indexed spellings rather than a pattern language. **This one needs a
   version decision**: a new field is additive and stays inside envelope 1, but a consumer that
   does not know the field still rejects the deployment, so it may need a `schema_version` bump to
   3 for a consumer to gate on. Decide it explicitly and write the reasoning into `FORMAT.md`;
   do not let it be settled by whichever field lands first.
2. **Whether `text_form` needs a value for a fixed-point or decimal type.** A 64-bit float is
   `unknown` in Rust because no measured pattern describes what its parse accepts. A Java
   implementation over `BigDecimal` may be able to say more — and if it can, it should, because
   `unknown` costs every range check downstream.
3. **Whether `producer` should carry the loader's version.** A binder that relaxes its parsing in a
   minor release silently invalidates every pattern measured before it. Against it: a third version
   string in one document, consumed by nothing. Two producers wrapping versioned libraries is the
   evidence this question was waiting for.

Each answer follows the order in [`spec/README.md`](../spec/README.md) under *Changing the spec*:
`FORMAT.md` first, then `contract.schema.json`, then re-bless the corpus and read the diff, then say
which tier the change affects.

---

## Phase 5 — beyond the producer

Not scheduled. Listed so it is visible that the migration does not end at a conforming document.

- A **Maven plugin** producing the contract at build time — the Java equivalent of
  [`rust/src/schema/cli/`](../rust/src/schema/cli/), whose whole argument is that every service was
  otherwise writing the same two-hundred-line generator and three of them ended up disagreeing
  about how to cut a `LABEL` block out of a Dockerfile.
- The **`verify` half** — checking a built image against what was generated. Rust has it in
  `schema/cli/verify.rs`; a chart's CI job in a Java shop needs it too.
- **Reload** for the vanilla loader: rebuilding a running service when the files under it change.
  The Rust `reload` feature deliberately does not depend on `loader`, and the Java port should keep
  that independence.
- **`explain`** — reporting which layer supplied each key.

---

## Conventions for the Java code

These are the house rules, and they are not negotiable per-file.

- **No streams.** Loops. This applies to the processor and the model walks as much as anywhere.
- **Lombok** for the model classes — `@Value`, `@Builder`, `@RequiredArgsConstructor`. The contract
  model is a large family of immutable value types and writing them by hand is how one of them ends
  up with an `equals` that skips a field.
- **Separation of concerns.** `-core` knows about documents, not about binders. `-loader` and
  `-spring-boot` know about their binder and produce core's model. The TCK knows about neither and
  compares documents. A Spring import in `-core` is a design failure, not a shortcut.
- **Minimal dependencies.** Jackson for JSON, networknt for schema validation, a TOML parser for
  the file layer, Lombok. Every addition beyond that needs an argument, and `-annotations` takes
  none at all.
- **Deterministic by construction**, not by test. Ordered collections in the render path, explicit
  property order, no reliance on annotation-processing round order for output ordering.
- **Comments explain why.** The Rust tree's comments are load-bearing — the `figment/test` note in
  `Cargo.toml`, the `DirEntry::metadata()` note in the secrets provider — and the Java side is
  held to the same standard. A comment restating the code is noise; a comment recording why the
  obvious thing is wrong is the reason the next person does not undo the fix.

---

## Verification commands

Rust, after Phase 0:

```bash
cd rust && cargo test --workspace --all-features
```

```bash
cd rust && cargo test --all-features --test spec
```

Re-blessing the corpus, which is what produces the diff every other implementation must match:

```bash
cd rust && TERRACE_SPEC_BLESS=1 cargo test --all-features --test spec
```

Java, from Phase 1:

```bash
cd java && mvn -B verify
```

Regenerating the measured reads tables in `spec/v1/FORMAT.md`:

```bash
cd java && mvn -B test -Dterrace.reads.bless=true
```

---

## The meta-schema's required fields, for reference

Reproduced so the core model can be checked against them without re-reading the schema. The
schema is authoritative; if these disagree, the schema is right.

| Object | Required |
|---|---|
| root | `terrace_contract`, `producer`, `app`, `schema`, `json_schema`, `external` |
| `producer` | `name`, `version`, `loader` |
| `app` | `name` |
| `schema` | `schema_version`, `dialect`, `loader`, `keys` |
| `dialect` | `prefix`, `nesting_separator`, `indirection_suffix` — all non-empty strings |
| `loader[]` | `env`, `role`, `docs`, `default` |
| `keys[]` | `path`, `env`, `env_file`, `secrets_file`, `docs`, `ty`, `values`, `text_form`, `aliases`, `env_aliases`, `env_file_aliases`, `secrets_file_aliases`, `default`, `default_value`, `note`, `required`, `secret`, `reserved` — and `unreachable` whenever `env` is null |
| `json_schema` | `$schema` |
| `external` | `env`, `ignore`, `unknown` |
| `external.env[]` | `name`, `docs`, `values`, `text_form`, `required`, `secret` |

The schema is deliberately **open** at every object level: it checks that what is present is
well-formed, not that nothing else is. A producer emitting a field nobody asked for is caught by the
corpus diff, not here.

---

## Risks

| Risk | Where it bites | Answer |
|---|---|---|
| The shared composite actions take no working directory | PR 1, before any code moves | Resolve upstream first. Treat it as a prerequisite, not a discovery. |
| A git-dependency consumer cannot resolve a crate in a subdirectory | After the first tag following PR 1 — i.e. after it is too late | The permanent consumer smoke-test job in PR 1. |
| release-please cannot vary `include-component-in-tag` per package | PR 2, silently, by tagging `rust-v0.12.0` and orphaning every existing pin | Verify against the installed release-please schema before merging. Fall back to a single Java component. |
| The Spring producer ships without the `_FILE` layer | PR 7, and never fails a test — the document is well-formed and describes a loader that does not exist | The `EnvironmentPostProcessor` is part of PR 7, not a follow-up. |
| Measured reads drift from the binder after a dependency bump | Any Renovate PR touching Spring or the TOML parser | Generate the tables from the measurement, so a bump that changes a read fails the build that proposes it. |
| Tier claimed above tier delivered | Anywhere | `CONFORMANCE.md`: claiming a tier you do not meet is worse than claiming none. The TCK enforces the claim; keep the claim in code, not only in prose. |
| Phase 0 lands while Phase 3 is in flight | Merge conflicts across every path in the tree | Phase 0 is two PRs and they merge before anything under `java/` is written. Do not parallelise across the move. |

---

## Open, and deliberately not decided here

- **Build tool.** Maven was chosen without being asked. Revisit before PR 3 lands; after it, the
  coordinates are published and the cost rises sharply.
- **Whether the Java artefacts are published**, and where. The crate is `publish = false` and
  distributed by git tag; Java has no equivalent idiom, and consuming a Maven module by git is
  materially worse than consuming a crate that way. This needs an answer before Phase 3 ends, and
  it may be the thing that argues for GitHub Packages or Maven Central.
- **Whether `rust/docs/config-contract-plan.md` belongs under `rust/`.** It is design, not crate
  documentation, and the argument for moving it to root `docs/` is decent. Left with the crate
  because it is the design that produced the crate, and because splitting it costs a link audit for
  no present benefit.
