# Migration progress

Tracks the state of the move to a multi-language repository (Rust under `rust/`, `spec/` staying
at the root, Java implementations to follow under `java/`). Updated as work lands; each entry
states what changed and what is still open, so a later session can pick up here without needing
the history that produced it.

## Done

- **The crate moved under `rust/`.** `src/`, `macros/`, `tests/`, `examples/`, `fuzz/`,
  `Cargo.toml`, `deny.toml`, `CHANGELOG.md` and `README.md` now live there. `docs/` split: the
  crate's own documents (`ALTERNATIVES.md`, `CONTRACT.md`, `EXPLAIN.md`, `RELOAD.md`, `SCHEMA.md`,
  `TESTING.md`, `config-contract-plan.md`) moved to `rust/docs/`; this file and other repo-level
  documents stay at the root. `spec/`, `LICENSE`, `SECURITY.md`, `.github/`, `.gitignore`,
  `.gitattributes` and `renovate.json` did not move.
- **`rust/tests/spec.rs`** — `spec_dir()` now climbs one level from `CARGO_MANIFEST_DIR` before
  joining `spec/v1`, since the manifest directory is `rust/` and not the repository root.
- **Cross-references updated** in `rust/docs/CONTRACT.md`, `spec/README.md`,
  `spec/v1/FORMAT.md` and `spec/v1/CONFORMANCE.md` to point through `rust/` where the target
  moved.
- **CI (`.github/workflows/ci.yml`)** — every Rust job (`fmt`, `clippy`, `test`, `features`,
  `doc`, `fuzz`, `deny`, `msrv`) now runs against `rust/` (via `working-directory`,
  `manifest-path`, or an explicit `cd`-equivalent). A new **`consumer-smoke-test`** job builds a
  throwaway crate depending on the checkout by git URL and is wired into the aggregate `ci` gate
  permanently, so a subdirectory package that stops resolving fails CI instead of being discovered
  after a tag.
- **`.github/workflows/update-files.yaml`** — lockfile resolution now targets
  `rust/Cargo.toml` and `rust/fuzz/Cargo.toml`, and the commit step's file pattern matches.
- **`.github/workflows/docs.yml`** and **`.github/scripts/readme-variables.sh`** — the manifest
  read defaults to `rust/Cargo.toml`, and the README render/check targets `rust/README.md`.
- **`.github/templates/README.md.hbs`** — now the crate template rendering to
  `rust/README.md`; its MSRV badge, `spec/`, `LICENSE`, `SECURITY.md` and `ci.yml` links were
  re-pointed for that new location.
- **Release automation** — `release-please-config.json` and `.release-please-manifest.json`
  key the Rust package as `"rust"` instead of `"."`, keeping `include-component-in-tag: false` so
  released tags stay `vX.Y.Z`.
- **`renovate.json`** — the fuzz-dependency grouping rule now matches
  `rust/fuzz/Cargo.toml`.
- **Verified locally**, from `rust/`: `cargo fmt --all --check`, `cargo clippy --workspace
  --all-features --all-targets`, `cargo test --workspace --all-features`, and
  `cargo test --all-features --test spec` all pass, including the corpus-vs-render comparison that
  proves the format still resolves `spec/v1/` correctly from the new manifest directory.

## Also done (this session)

- **Fixed stale links left by the move.** `.github/templates/README.md.hbs` re-pointed its MSRV
  badge at `../rust/Cargo.toml`, which was wrong — the badge lives in `rust/README.md`, right
  beside `rust/Cargo.toml`, so the link needed no prefix at all. Its Licence badge had the opposite
  problem: still pointing at bare `LICENSE` when `LICENSE` moved one level up relative to the new
  `rust/README.md`. Both are fixed, and the *committed* `rust/README.md` was hand-synced to match
  (it had drifted from the template edits made earlier in the move, which only changed the
  template, not the rendered file).
- **Root `README.md` and its template.** `.github/templates/root-README.md.hbs` renders the
  repository's front door: what the project is, an implementations table (Rust shipping, both
  Java rows marked "not started" with no link since `java/` does not exist), and one link each
  into `spec/README.md` and `rust/README.md`. `docs.yml`'s `render` and `check` jobs now render
  and verify both `rust/README.md` and the root `README.md` from the same payload. The rendered
  `README.md` is committed now rather than left absent, since CI cannot render it outside a
  pull request.
- **`docs/CONTRIBUTING.md`** — commit message convention, the rule that a spec change and every
  corpus update it causes ship in one pull request, and where root-level vs. per-implementation
  documents belong.
- **`renovate.json`** — a packageRule scoping the Gradle manager to `java/**/build.gradle.kts`
  and the version catalog, with test dependencies grouped the way `fuzz/` already is (superseded
  later this session when the build tool changed — see "Build tool switched to Gradle" below).

## Java skeleton and the TCK (this session)

- **`java/` exists now**, originally as a Maven aggregator (Java 21 release, `dev.terrace`
  groupId, `0.1.0-SNAPSHOT`) with six modules: `terrace-config-annotations`, `terrace-config-core`,
  `terrace-config-processor`, `terrace-config-loader`, `terrace-config-spring-boot` and
  `terrace-config-spec-tck`. Only the last has any code; the other five are empty jars stating
  what they will be and which upcoming pull request fills them in. **Rebuilt on Gradle later this
  session — see "Build tool switched to Gradle" below; the module list and scope are unchanged.**
- **`terrace-config-spec-tck` is implemented and tested**, before any producer exists, which was
  the point: `MetaSchemaValidator` compiles `spec/v1/contract.schema.json` (networknt
  `json-schema-validator`, draft 2020-12, no network resolution needed since the meta-schema is
  self-contained) and exposes both entry points the Rust suite checks — the full envelope, and the
  `schema` object alone against `#/$defs/schema` for the `json` rendering published on its own.
  `TierComparator` compares two documents at tier 1 (producer block present), tier 2 (adds
  field-for-field equality of `env`, `env_file`, `secrets_file`, the three alias lists and
  `unreachable` for every key), and tier 3 (byte equality after substituting `producer.version`
  with `0.0.0-conformance`, mirroring `CONFORMANCE_VERSION` in `rust/tests/spec.rs` exactly).
  Output is a list of readable, path-qualified diff strings, not an assertion message.
- **Tests, all passing locally**: every stored corpus case (`minimal`, `full-surface`,
  `unnameable-key`) validates against the meta-schema, and each case's `schema` half validates
  against the schema-only sub-schema on its own — mirroring
  `the_stored_corpus_satisfies_the_meta_schema` and
  `the_schema_half_satisfies_the_meta_schema_on_its_own` in the Rust suite. `TierComparator` is
  proved against the same stored cases: a document compared with itself satisfies every tier, and
  three targeted mutations (a changed field, a flipped `unreachable`, a blanked `producer.loader`)
  are caught at exactly the tier that field belongs to.
- **`java/README.md`** — the module table above, build instructions, and the house conventions (no
  streams, Lombok once the model exists, minimal dependencies, deterministic rendering); now
  updated for Gradle (see below).
- A local build tool install was needed to verify any of this in both attempts — see the Maven
  and Gradle notes below for what each required in a fresh environment.

## Build tool switched to Gradle (this session)

- **All six `pom.xml` files removed**, replaced with Gradle (Kotlin DSL): `java/settings.gradle.kts`
  (module list, unchanged), `java/gradle/libs.versions.toml` (a version catalog taking over the
  role of the aggregator's `<dependencyManagement>` — same versions: JUnit 5.11.4, AssertJ 3.27.3,
  Jackson 2.18.3, Lombok 1.18.36, networknt json-schema-validator 1.5.5, Spring Boot 3.4.2,
  compile-testing 0.21.0), and `java/build.gradle.kts` (shared config: Java 21 toolchain, UTF-8,
  JUnit Platform for every module).
- **Every module's dependency list carried over exactly**, including the Maven scope quirks that
  mattered: Lombok's `provided` scope became `compileOnly` + `annotationProcessor` in
  `terrace-config-core`; `terrace-config-processor`'s `provided` dependency on itself from
  `terrace-config-loader` became `compileOnly` + `annotationProcessor` on the project dependency.
  `terrace-config-annotations` still takes nothing beyond the JUnit/AssertJ every module gets,
  matching its zero-dependency design.
- **Why switch:** requested directly, ahead of anything depending on the earlier Maven choice
  being final (the plan itself had flagged the choice as unasked-for and to be revisited before
  PR 3 landed — since nothing outside `java/` referenced Maven specifics, the switch stayed
  contained to this directory, `renovate.json`, and this document).
- **The Gradle wrapper is committed** (`gradlew`, `gradlew.bat`, `gradle/wrapper/`), pinned to
  Gradle 9.1.0 — needed because the only Java available in this environment is 25, and Gradle
  did not support running its daemon on Java 25 until 9.1.0 (8.x fails outright with
  `IllegalArgumentException: 25.0.3`). A fresh environment with an older JDK can still use the
  same wrapper; the pin is a build-tool constraint, not a target constraint (module compilation
  still targets Java 21 via the toolchain).
- **Verified**: `./gradlew test` from `java/` builds all six modules and runs
  `terrace-config-spec-tck`'s suite — 13/13 tests green, identical result to the Maven run it
  replaced.
- **`java/README.md`** — build instructions now say `./gradlew test`; mentions the wrapper needs
  no local Gradle install and where module versions now live.
- **`.gitignore`** — added a Gradle section (`build/`, `.gradle/`) mirroring the existing Cargo
  `target/` entry; the wrapper jar itself is committed, only its ad hoc distribution cache is not.
- **`renovate.json`** — the packageRule from the previous session (scoped to Maven's `pom.xml`)
  replaced with one scoped to Gradle's `build.gradle.kts` and the version catalog, same grouping
  intent. Still inert: nothing depends on anything yet beyond the fixed versions above.
- Getting a working local Gradle needed the same kind of detour Maven did: `services.gradle.org`
  resolved directly (unlike `dlcdn.apache.org` for Maven), but the first distribution tried
  (8.12) didn't support the installed Java 25 daemon; 9.1.0 was needed. Noted here so a future
  session doesn't re-discover this from scratch.

## Lombok and the Jackson 2 vs. 3 split (this session)

- **Lombok is wired in project-wide, not just declared in the version catalog.** The
  io.freefair.lombok Gradle plugin (version 9.1.0, matching the Gradle wrapper's own minor
  version per that plugin's compatibility policy) is declared once, `apply false`, in the root
  `java/build.gradle.kts`, then applied with no version repeated in `terrace-config-core`,
  `terrace-config-processor`, `terrace-config-loader` and `terrace-config-spring-boot` — the four
  modules that have, or will have, bean-shaped classes. `terrace-config-core`'s previous
  hand-rolled `compileOnly` + `annotationProcessor` lines are gone; the plugin now does that for
  both main and test sources, and additionally points `javadoc` at delomboked output
  automatically. `terrace-config-annotations` (annotation types can't carry Lombok annotations;
  the module's whole point is "nothing but the JDK") and `terrace-config-spec-tck` (its classes
  predate the plugin and aren't bean-shaped) deliberately opt out, each with a comment in its
  `build.gradle.kts` saying so.
- **A shared `java/lombok.config`** (picked up by Lombok itself by walking up from each source
  file, `config.stopBubbling = true` stopping that walk at `java/`) sets `@Generated` marking on
  generated members (so coverage tooling skips them), `callSuper = call` on
  `@EqualsAndHashCode`/`@ToString` (fails the build instead of silently ignoring superclass
  fields), and private-by-default field visibility — deliberately *not* a final-by-default field
  policy, since whether a given PR-4-onward model class is immutable is a per-class decision, not
  one to bake into shared config ahead of that class existing.
- **The Jackson 2 vs. 3 question, and why no dual-support scheme is needed.** Jackson 3.0 (GA
  October 2025, driven by Spring Boot 4's own move to it) renamed its group ID and packages from
  `com.fasterxml.jackson` to `tools.jackson` — except `jackson-annotations`, kept on
  `com.fasterxml.jackson` 2.x so both majors share it. Because of that rename, a 2.x and a 3.x
  Jackson artifact are different Maven coordinates, not two versions Gradle has to reconcile —
  they coexist on one classpath without conflict. That removes the need for a version-alignment
  scheme across most of `java/`:
  - `terrace-config-core` keeps a direct, version-catalog-pinned dependency on `jackson-databind`
    2.x, purely as an internal detail of its own JSON rendering, never exposed on a public API —
    so it never has to track whatever Jackson major a consumer uses elsewhere.
  - `terrace-config-spring-boot` declares no Jackson dependency at all; it only reaches JSON
    through Spring's own `Binder`/`ObjectMapper`, so it automatically follows whichever Jackson
    major the consuming app's Spring Boot BOM resolves. This is the one module the split can
    actually reach, since it is the one module bound to Spring's own Jackson instance.
  - The plan for actual Spring Boot 4 / Jackson 3 support, when needed, is a sibling module
    (`terrace-config-spring-boot4`) with its own Boot 4 BOM pin, mirroring how Boot 3 vs. 4 already
    diverge upstream, rather than a runtime check or a shaded dependency inside
    `terrace-config-spring-boot`. Not built this session — nothing depends on it before
    `terrace-config-spring-boot` itself (PR 7) exists to fork from.
  - Full write-up: `java/README.md`, "Lombok" and "Jackson: handling the 2.x / 3.x split"
    sections.
- **Verified**: `./gradlew clean test` from `java/` still builds all six modules and passes
  `terrace-config-spec-tck`'s 13/13 tests after the Lombok rewiring.

## Shared build logic moved to buildSrc (this session)

- **`java/buildSrc/` added** as its own standalone Gradle build hosting two precompiled script
  ("convention") plugins under `buildSrc/src/main/kotlin/`: `terrace-config.java-conventions`
  (the Java 21 toolchain, UTF-8 encoding, JUnit 5 + AssertJ test dependencies) and
  `terrace-config.lombok-conventions` (the above, plus `io.freefair.lombok` pre-configured with
  the version from the catalog). This replaces the root `java/build.gradle.kts`'s `subprojects
  { ... }` block from the previous session with the same settings expressed as plugins each
  module opts into by ID; the root `java/build.gradle.kts` is now just `allprojects { group =
  ...; version = ... }`.
- **Every non-empty module updated**: `terrace-config-annotations` and `terrace-config-spec-tck`
  now apply only `terrace-config.java-conventions` (unchanged reasons for skipping Lombok, moved
  from a `plugins`-block comment to referencing the shared plugin instead); `terrace-config-core`,
  `terrace-config-processor`, `terrace-config-loader` and `terrace-config-spring-boot` now apply
  `terrace-config.lombok-conventions` in place of their own `id("io.freefair.lombok")` +
  `lombok { version.set(libs.versions.lombok) }` pair.
- **`buildSrc/settings.gradle.kts`** points buildSrc's own version catalog at the same
  `java/gradle/libs.versions.toml` file the modules use (`dependencyResolutionManagement.
  versionCatalogs.create("libs") { from(files("../gradle/libs.versions.toml")) }`), and a new
  `lombok-plugin` version entry was added to that file for the `io.freefair.gradle:lombok-plugin`
  artifact `buildSrc/build.gradle.kts` depends on — kept distinct from the `lombok` entry, which is
  the Lombok *library* version, not the Gradle plugin's own version.
- **Type-safe `libs.xxx` catalog accessors do not work inside a precompiled script plugin** — a
  documented Gradle limitation (the plugin might end up applied to a project with a
  differently-shaped catalog), discovered when the first attempt at both convention plugins failed
  to compile with `Unresolved reference 'libs'`. Fixed by using the lower-level, string-keyed
  `VersionCatalogsExtension` API instead (`extensions.getByType(VersionCatalogsExtension::class
  .java).named("libs")`, then `.findLibrary("...")`/`.findVersion("...")`). Ordinary module
  `build.gradle.kts` files are unaffected and keep using the type-safe accessors, since they are
  regular project build scripts, not precompiled plugins.
- **Verified**: `./gradlew clean test` from `java/` builds all six modules (`buildSrc` compiles
  first) and passes `terrace-config-spec-tck`'s suite — 7/7 in `MetaSchemaValidatorTest` and 6/6
  in `TierComparatorTest`, 13/13 total, matching every prior run.
- Full write-up: `java/README.md`, new "buildSrc" section, plus the updated "Lombok" section.

## Java package/group renamed to de.timscho (this session)

- **Every Java path under `java/` now uses `de.timscho`, not `dev.terrace`.** The only module with
  real source, `terrace-config-spec-tck`, had its five main-source classes
  (`ConformanceVersion`, `MetaSchemaValidator`, `SpecPaths`, `Tier`, `TierComparator`) and two test
  classes (`MetaSchemaValidatorTest`, `TierComparatorTest`) moved with `git mv` from
  `src/{main,test}/java/dev/terrace/config/tck/` to `src/{main,test}/java/de/timscho/config/tck/`,
  with each file's `package dev.terrace.config.tck;` line updated to
  `package de.timscho.config.tck;` to match. The other five modules
  (`terrace-config-annotations`, `-core`, `-processor`, `-loader`, `-spring-boot`) have no source
  files yet, so there was nothing to move there — the new package prefix simply applies from
  their first real class onward.
- **`java/build.gradle.kts`'s `group = "dev.terrace"` changed to `group = "de.timscho"`** in the
  shared `allprojects` block, so every module's coordinate picks up the new group automatically.
- **Deliberately left untouched**: the `dev.terrace.config.*` OCI label strings used throughout
  `spec/v1/FORMAT.md`, `rust/docs/CONTRACT.md`, `rust/src/schema/contract.rs`, and the Rust test
  suite. Those are container-label vocabulary defined by the spec itself, not a Java package or
  build coordinate, so they are out of scope for this rename.
- **Verified**: `./gradlew clean test` from `java/` after the rename still builds all six modules
  and passes `terrace-config-spec-tck`'s full suite (7/7 `MetaSchemaValidatorTest`, 6/6
  `TierComparatorTest`, 13/13 total), confirming the move and package edits didn't break anything.

## terrace-config-core implemented: the envelope model and the eight refusals (this session)

- **The document model** now exists as real Java, one Lombok `@Value @Builder @Jacksonized` class
  per meta-schema object under `de.timscho.config.core.model` — `Contract` (root), `Producer`,
  `App`, `Schema`, `Dialect`, `LoaderVar`, `Key`, `External`, `ExternalVar` — plus four enums
  (`LoaderRole`, `TextForm`, `UnreachableReason`, `ExternalUnknownPolicy`) and one hand-written
  `JsonSchemaDocument` wrapping an order-preserving map for the deliberately open `json_schema`
  half. Every class carries an explicit `@JsonPropertyOrder` mirroring `contract.schema.json`'s
  own property order, and `Key`/`ExternalVar` carry per-field `@JsonInclude` overrides, because the
  meta-schema's `required` list and "open at every level" shape don't line up field for field
  (e.g. `ty` is required but nullable; `constraint` is genuinely optional).
- **A byte-stable JSON codec**, `de.timscho.config.core.io.ContractCodec`, the only place an
  `ObjectMapper` is built. Its `CompactEmptyContainerPrettyPrinter` fixes the two ways Jackson's
  own `DefaultPrettyPrinter` disagrees with every `contract.json` in the corpus: it renders an
  empty array as `[ ]` (corpus: `[]`) and a field separator as `"key" : value` (corpus: `"key":
  value`). `spec/v1/FORMAT.md`'s "Publication" section defines the document as byte-stable, so
  this needed a real fix, not a rounding-error tolerance in the test.
- **The eight build-time refusals** from `spec/v1/FORMAT.md`'s "What a producer MUST refuse", each
  its own `ContractRefusalException` subclass under `de.timscho.config.core.refusal`
  (`EmptyPrefixException`, `SecretWithDefaultException`, `ExternalVariableInPrefixException`,
  `IgnorePatternInPrefixException`, `ExternalVariableCollisionException`,
  `IgnorePatternCollisionException`, `DuplicateExternalVariableException`,
  `IndirectionCollisionException`), run in order by `ContractValidator.validate(Contract)`. Lives
  beside the model rather than inside a loader, since both `-loader` and `-spring-boot` will build
  the same `Contract` shape and must refuse the same eight things.
- **Tested against the corpus before any producer exists**, exactly as PR 4 calls for:
  `ContractCorpusRoundTripTest` deserialises every stored `spec/v1/conformance/*/contract.json`
  into `Contract` and re-serialises it, asserting byte-for-byte equality with the file on disk —
  the single test that exercises the whole model and every property-ordering decision at once.
  `ContractValidatorTest` has one test per refusal (plus one proving a well-formed contract passes
  every check), each built from a minimal hand-constructed `Contract` so the refusal under test
  stays isolated from the other seven — in particular, refusals #3 and #4 needed a loader
  variable deliberately spelled *outside* the dialect prefix (`CREDENTIALS_DIR`, matching
  `FORMAT.md`'s own example) so they don't just re-trigger refusal #1/#2 on the same fixture.
- **Not attempted this session, and out of scope for PR 4's own text**: the renderings beyond
  `json` (`markdown`, `markdown-loader`, `markdown-keys`, `toml`, `json-schema`, `contract`,
  `labels`, `dockerfile`), since PR 4 explicitly allows the markdown/toml trio and `contract`'s
  sibling renderings to land in a follow-up if the PR is large enough already — which this one is.
- **Verified**: `./gradlew clean test` from `java/` builds all six modules; `terrace-config-core`
  passes 15/15 tests (4 corpus round-trips + 11 validator tests, one being the discovery check for
  the two conformance directories); `terrace-config-spec-tck` is unaffected, still 13/13.
- Full write-up: `java/README.md`'s new "`terrace-config-core`" section.

## Dual Jackson 2 / 3 support implemented (this session)

The earlier "Lombok and the Jackson 2 vs. 3 split" session above planned to keep `-core` pinned to
Jackson 2 and only let the split surface in a future `terrace-config-spring-boot4`. Asked
explicitly to support both now, that plan changed: the model itself works under both majors, not
just one, before any Spring module exists.

- **Removed the databind dependency from the model.** `Dialect`, `Schema`, `ExternalVar` and `Key`
  used a class-level `@JsonNaming(PropertyNamingStrategies.SnakeCaseStrategy.class)`, which lives
  in `com.fasterxml.jackson.databind.annotation` -- a Jackson-2-only package with no Jackson-3
  equivalent import path. Replaced with an explicit `@JsonProperty("snake_case_name")` per field
  (`com.fasterxml.jackson.annotation`, the one package both majors share), so the model compiles
  and renders identically either way.
- **`@Jacksonized` now emits both majors' annotations.** Lombok 1.18.44 added
  `lombok.jacksonized.jacksonVersion`, letting `@Jacksonized` generate
  `@JsonDeserialize`/`@JsonPOJOBuilder` for Jackson 2 (`com.fasterxml.jackson.databind.annotation`)
  and Jackson 3 (`tools.jackson.databind.annotation`) on the same class, instead of one or the
  other. Bumped `lombok` in the version catalog from 1.18.36 to 1.18.46 and set
  `lombok.jacksonized.jacksonVersion += 2` / `+= 3` in `java/lombok.config`. `terrace-config-core`
  declares both `jackson-databind` (2.x) and the new `jackson-databind-v3` (`tools.jackson.core:
  jackson-databind:3.0.0`) catalog entries as `compileOnly` -- needed only so those emitted
  annotation types resolve at compile time, never as a runtime requirement of the model itself.
- **The codec moved out of `-core` into two new sibling modules.** `ContractCodec` and
  `CompactEmptyContainerPrettyPrinter` (`git mv`'d, unchanged) now live in
  `terrace-config-core-jackson2`; a new `terrace-config-core-jackson3` provides the same two
  classes rebuilt on `tools.jackson.*`. Both wired into `settings.gradle.kts`, both depend on
  `-core` plus their own Jackson major only -- so depending on `-core` alone pulls in neither
  Jackson artifact, and a consumer adds exactly one codec module.
- **Jackson 3 API differences hit while porting the codec**: `PrettyPrinter`'s
  `writeObjectFieldValueSeparator` was renamed `writeObjectNameValueSeparator` (field to name);
  `ObjectMapper` is now built once, immutably, through `JsonMapper.builder()...build()` rather than
  constructed then mutated with `enable`/`disable`; and the streaming API throws the unchecked
  `JacksonException` instead of `IOException`, so the Jackson 3 `ContractCodec` needed fewer
  `try`/`catch` blocks around the actual (de)serialisation calls than its Jackson 2 twin.
  `terrace-config-core-jackson3`'s own `ContractCorpusRoundTripTest` (a copy of `-jackson2`'s)
  proves the exact same `-core` model classes really do round-trip byte-for-byte under Jackson 3,
  not just Jackson 2.
- **A `compileOnly`/test regression, found and fixed**: `-core`'s `compileTestJava` started
  emitting "unknown enum constant" warnings for `JsonInclude.Include` once the databind
  dependencies became `compileOnly`, since the `java` plugin does not extend `testCompileOnly` from
  `compileOnly` automatically. Fixed with one line,
  `configurations.testCompileOnly.get().extendsFrom(configurations.compileOnly.get())`, in
  `terrace-config-core/build.gradle.kts`.
- **Verified**: `./gradlew clean test` from `java/` -- `BUILD SUCCESSFUL`, zero warnings, all 8
  modules (including the two new codec modules) build; `terrace-config-core-jackson2` and
  `-jackson3` each pass their own 4-case `ContractCorpusRoundTripTest` plus the shared
  `discoversAtLeastTheThreeKnownCases` check; `terrace-config-core` still passes its 11
  `ContractValidatorTest` cases; `terrace-config-spec-tck` unaffected at 13/13.
- Full write-up: `java/README.md`'s rewritten `terrace-config-core` section, new
  `terrace-config-core-jackson2` / `-jackson3` section, and the replaced "Dual Jackson 2 / 3
  support" section (previously "Jackson: handling the 2.x / 3.x split").

## `terrace-config-annotations` and `terrace-config-processor` implemented (this session)

`@TerraceConfig` and the annotation vocabulary, plus the JSR-269 processor that reads it -- the
Java answer to `#[derive(Describe)]` -- are implemented and tested, ahead of any loader or Spring
producer that would consume them, the same "harness/vocabulary before the thing that needs it"
ordering the earlier TCK and core sessions followed.

- **The annotation vocabulary** (`terrace-config-annotations`, zero dependencies, unchanged):
  `@TerraceConfig` (class/record or enum), `@Nested`, `@Secret`, `@Skip`, `@Note`, `@Values`
  (bare / `from = X.class` / a literal list), `@ElementValues` (the same, one container level
  down), `@Element` (a container's element is itself a nested struct), `@Range` (`min`/`max`/
  `exclusiveMin`/`exclusiveMax`, `Double.NaN` marking an unset end). All `@Retention(SOURCE)` --
  read by the processor off the compiler's own AST during annotation processing, never by runtime
  reflection.
- **A dialect-agnostic descriptor model** was added to `terrace-config-core`
  (`de.timscho.config.core.descriptor`: `TypeDescriptor`, `KeyDescriptor`, `ElementDescriptor`,
  `RangeConstraint`, all plain Java records -- no Lombok needed). Deliberately carries no
  environment spelling, alias list or `text_form`: those are dialect decisions a loader or Spring
  producer makes later, combining a descriptor with its own dialect to build the actual `Key`
  model. This is why `terrace-config-processor` depends on `-core` (to reuse this model) but
  `-core` still knows nothing about annotations or the processor.
- **`TerraceConfigProcessor`** generates one `<Type>Descriptor` class per `@TerraceConfig`-annotated
  type, exposing `public static final TypeDescriptor DESCRIPTOR`. Field resolution mirrors
  `rust/docs/SCHEMA.md` closely: a recognised leaf (`String`, the primitives/boxes, `BigInteger`,
  `BigDecimal`) needs no annotation; `Optional`/`List`/`Set`/`Map` are unwrapped to find the
  element (a map's *value*, never its key); and a field or element that is none of those and
  carries none of the resolving annotations is a **compile error** naming the field, its type, and
  the attributes that would resolve it -- word for word the diagnostic `SCHEMA.md` calls "the whole
  point of the feature". `com.fasterxml.jackson.annotation.JsonIgnoreProperties`/`@JsonProperty`
  are read off `AnnotationMirror` by qualified name (never imported), so the processor takes no
  compile dependency on either Jackson major.
- **A real bug, found and fixed before it shipped**: a `@TerraceConfig` type nested inside another
  class (e.g. `Outer.Inner`) cannot have its descriptor named by dot-concatenation --
  `Filer.createSourceFile("test.Outer.InnerDescriptor", ...)` asks for a class in a *package*
  `test.Outer`, which doesn't exist. Fixed with `DescriptorNaming`, joining every enclosing simple
  name with `$` into one top-level class (`Outer$InnerDescriptor`) living in the annotated type's
  real package -- used consistently by both the top-level generator and cross-descriptor references
  (`@Nested`/`@Values`/`@Element`/`@ElementValues` pointing at another annotated type).
- **`terrace-config-processor` needs no Lombok** -- its own code is plain
  `javax.annotation.processing`/`javax.lang.model` logic, and the descriptors it emits reference
  `-core`'s plain records, never a Lombok-annotated class of its own -- so it applies
  `terrace-config.java-conventions` rather than the `lombok-conventions` its `build.gradle.kts` was
  scaffolded with in an earlier session.
- **Tested with `compile-testing`** (`TerraceConfigProcessorTest`, 15 cases): one small in-memory
  compilation per resolvable shape (plain leaves, `@Nested`, all three `@Values` forms, `@Range`,
  `@Element`, `@ElementValues`, `@Skip`) asserting the compilation succeeds and the expected
  descriptor is generated, and one per refusal (unresolved leaf, `@Range` on a non-numeric type,
  `@Range` with no bound set, `@Nested` on a non-`@TerraceConfig` type, an unresolved container
  element, conflicting shape annotations on one field) asserting the diagnostic text.
- **Verified**: `./gradlew clean test` from `java/` -- `BUILD SUCCESSFUL`, all 8 modules build;
  `terrace-config-processor` passes 15/15; every other module's suite (`-core`, `-core-jackson2`,
  `-core-jackson3`, `-spec-tck`) unaffected at its prior count.
- Full write-up: `java/README.md`'s new "`terrace-config-annotations` and
  `terrace-config-processor`" section, and updated module table/Lombok section.

## `terrace-config-loader` implemented: the vanilla five-layer loader (this session)

`terrace-config-loader` is a working, tested Java port of the Rust crate's `Terrace` loader --
struct defaults, a TOML file/directory, prefixed environment variables, a secrets directory, and
`_FILE` indirection -- rather than the empty skeleton it was.

- **`de.timscho.config.loader.Dialect`** ports `dialect.rs` field for field: prefix, `__`
  separator, `_FILE` suffix, and a reserved-key set, immutable with `with`-style methods.
  `keyPath`/`envSpelling` round-trip a key between its two spellings; the separator and
  reserved-key matching are both case-insensitive, for the same Windows-environment and
  mixed-case-secrets-file reasons the Rust doc comments give.
- **The three layers** each produce a nested `Map<String, Object>` instead of a
  `figment::Provider` (there is no figment on the JVM): `TomlLayers` (one file, or every `*.toml`
  in a directory, sorted and deep-merged), `SecretsDir` (a directory of key-named files, dotfiles
  and non-regular entries skipped, symlinks followed), `FileSuffixEnv` (`<PREFIX><KEY>_FILE=/path`
  indirection, scanned over the whole environment since the keys are open-ended). `LayerValues`
  holds the shared `insertNested`/`deepMerge`/`readValue` (UTF-8-strict, trailing `\r`/`\n`
  trimmed, never spaces) helpers.
- **`ShadowPolicy`** (`REJECT` default, `LAST_WINS`) and **`FileLayers`** are ported one for one
  from `layers.rs`, refusing under `REJECT` a key supplied by more than one of the environment,
  the secrets directory, or indirection, error messages included.
- **`TerraceLoader`** is the builder (`configVar`/`secretsDirVar`/`defaultConfigPath`/
  `fileSuffix`/`nestingSeparator`/`reserve`/`shadowPolicy`), merging TOML, then the prefixed
  environment (excluding reserved names and `_FILE` indirections), then the file-backed layers on
  top, into one nested map bound to the caller's type via a plain Jackson
  `ObjectMapper.convertValue` -- an internal detail, not exposed on the public API, exactly as
  `-core`'s own Jackson dependency is. `load(Class<T>, Map<String, String>)` takes an explicit
  environment instead of `System.getenv()`, which is the seam the test suite uses.
- **A real bug, found and fixed before it shipped**: `tomlj`'s `TomlTable.toMap()` does not
  recursively convert nested tables into plain `Map`s -- a nested `[section]` stayed a live
  `TomlTable` object, which Jackson then read through bean introspection instead of as a JSON
  object, surfacing as a spurious `"empty"` property (`TomlTable`'s own `isEmpty()`) on every
  contract with a non-flat TOML file. Fixed with `TomlLayers`' own recursive
  `TomlTable`/`TomlArray` -> `Map`/`List`/scalar converter built on the public `keySet()`/`get()`
  accessors, not on `toMap()`.
- **Tested with real files in a JUnit `@TempDir`**, not the corpus -- this loader has no
  `Contract`-shaped output of its own to compare against a stored case: `DialectTest` (8 cases,
  ported from `dialect.rs`'s own unit tests) and `TerraceLoaderTest` (8 cases: TOML alone, an
  environment variable overriding TOML, a secrets-directory file, `_FILE` indirection, a shadowed
  key rejected under `REJECT` and resolved under `LAST_WINS`, a reserved key refused from a
  secrets file, and a missing TOML file being silently skipped).
- **`org.tomlj:tomlj:1.1.1`** added to `gradle/libs.versions.toml` -- the first non-Jackson,
  non-test runtime dependency in `java/`.
- **Verified**: `./gradlew clean test` from `java/` -- `BUILD SUCCESSFUL`, all 8 modules build;
  `terrace-config-loader` passes 16/16 (8 `DialectTest` + 8 `TerraceLoaderTest`); every other
  module's suite unaffected at its prior count.
- **Not built this session, and open**: `schema()`/`schema_at()` (combining a `Dialect` with the
  processor's `TypeDescriptor` model to describe every key a type can carry, mirroring
  `Terrace::schema`), `explain()` (a provenance report independent of `ShadowPolicy`), and
  `load_watched` (the paths a reload should watch, plus a fingerprint). All three need the loader
  itself to exist first, which is what this session built; none change the five layers or their
  precedence.

## `terrace-config-spring-boot` started: `_FILE` indirection over Spring's own binder (this session)

`terrace-config-spring-boot` is no longer an empty skeleton, though the module is not yet the
tier-1 `Contract` producer PR 7 ultimately calls for — only the one layer Spring's own `Binder`/
`ConfigDataEnvironmentPostProcessor` genuinely lacks.

- **`de.timscho.config.spring.FileIndirectionEnvironmentPostProcessor`** implements Spring Boot's
  `EnvironmentPostProcessor`: it scans the environment's own `systemEnvironment` property source
  for `<NAME>_FILE=/path` variables, reads each named file (UTF-8-strict, trailing `\r`/`\n`
  trimmed, never spaces — the same rule `terrace-config-loader`'s `LayerValues.readValue` uses),
  and publishes the results as a new property source added at the *front* of the environment, so a
  file-backed value always outranks a plain environment variable or a configuration file already
  loaded — the same "file layers win" precedence `terrace-config-loader`'s `FileLayers` documents.
  Registered via `META-INF/spring.factories`, the mechanism `EnvironmentPostProcessor` still uses
  in Spring Boot 3 (unlike `@Configuration` auto-configuration, which moved to
  `AutoConfiguration.imports`), since it has to run before any bean — including an
  auto-configuration class — exists.
- **`de.timscho.config.spring.SpringDialect`** documents, and its own test exercises, the one real
  divergence from `terrace-config-loader`'s `Dialect`: Spring's relaxed binding already treats a
  single `_` as the environment's nesting separator, where the vanilla loader deliberately uses
  `__` precisely so a field's own name may contain an underscore without colliding with nesting.
  This module cannot change Spring's own convention, so the ambiguity (a field literally named
  `github_token` versus a nested `github.token` spelling identically as `GITHUB_TOKEN`) is real
  and permanent for this producer, not a bug to fix — made explicit rather than left implicit.
- **Why the `Contract` producer itself is still out of scope**: building one means combining
  `terrace-config-processor`'s `TypeDescriptor` model with a dialect to produce a `Schema`, the way
  `Terrace::schema` does in Rust — but a `Contract` also needs a populated `json_schema` half, and
  `terrace-config-core` does not implement that rendering yet (see "Open" below, carried over
  unchanged from PR 4). Publishing an envelope with an empty or invented `json_schema` would
  violate the format the eight refusals in `-core` exist to enforce, so the schema-description
  wiring is deferred until that rendering lands, exactly as `terrace-config-loader`'s own
  `schema()`/`explain()` equivalents already are.
- **Verified**: `./gradlew clean test` from `java/` — `BUILD SUCCESSFUL`, all 9 modules build;
  `terrace-config-spring-boot` passes 14/14 (7 `SpringDialectTest` + 7
  `FileIndirectionEnvironmentPostProcessorTest`, the latter using `spring-test`'s
  `MockEnvironment` rather than a booted `SpringApplication`, since `EnvironmentPostProcessor`s run
  before there is a context to boot); every other module's suite unaffected at its prior count.
- Full write-up: `java/README.md`'s new `terrace-config-spring-boot` section and updated module
  table.

## `terrace-config-core`'s `json-schema` rendering implemented (this session)

`Schema.toJsonSchema()`/`toJsonSchemaWith` render a `Schema` as the JSON Schema document an editor
or a Helm chart validates a rendered configuration against — a straight port of the Rust crate's
`schema::json_schema` module, the rendering both `terrace-config-loader`'s open `schema()` and
`terrace-config-spring-boot`'s open `Contract` producer were blocked on.

- **`de.timscho.config.core.schema`** gained four classes: `Docs` (how much of a key's comment
  becomes its `description`), `JsonSchemaOptions` (the dialect URI, `$id`, `title`, `closed`,
  `requirePresent`, `defaults` knobs — `forContract()` is the draft-07/closed/`requirePresent`-off
  preset the Rust crate's own `Schema::into_contract` uses for a `Contract`'s embedded half),
  `Node` (groups the flat, dotted `Key.path` list into the nested tree a `properties` object
  needs, ported from `tree.rs`), and `JsonSchemaRenderer` (`document`/`object`/`leaf`/`close`,
  ported from `json_schema.rs` field for field — alias handling, the `allOf`/`anyOf` case for a
  required key with aliases, and `close`'s recursion into `items`/`additionalProperties` rather
  than overwriting either).
- **Every map returned is a `TreeMap`**, so iteration order is already alphabetical — the same
  order `serde_json`'s own (non-`preserve_order`) map produces, which is what every stored
  `contract.json`'s `json_schema` half is written in. This sidesteps needing to reproduce Rust's
  exact map type: a corpus test can compare structurally rather than byte for byte.
- **`JsonSchemaRendererCorpusTest`**, added to `terrace-config-core-jackson2` (it needs a real
  `ObjectMapper` to parse the corpus, which `-core` itself deliberately has none of), re-renders
  each stored case's `schema` with `JsonSchemaOptions.forContract()` and asserts the result is
  structurally equal to that same case's stored `json_schema` field — proving the port against
  real reference output, not just hand-built fixtures, and with no Java producer needed to do it.
- **A real Lombok/Gradle interaction, found while adding this**: `Node`'s fields, written with no
  access modifier (intending ordinary package-private visibility), compiled as *private* and broke
  `JsonSchemaRenderer`'s access to them from the same package. Cause: `java/lombok.config`'s
  `lombok.fieldDefaults.defaultPrivate = true` (added in an earlier session, "Lombok and the
  Jackson 2 vs. 3 split") applies to every field with no explicit access modifier in any
  Lombok-processed module — not only fields on classes actually carrying a Lombok annotation, which
  `Node` does not. Fixed by giving `Node`'s three fields an explicit `public` modifier, harmless
  since `Node` itself is a package-private class.
- **Verified**: `./gradlew clean test` from `java/` — `BUILD SUCCESSFUL`, all 9 modules build;
  `JsonSchemaRendererCorpusTest` passes 3/3 (one per stored corpus case); every other module's
  suite unaffected at its prior count (`-core` 11, `-core-jackson2` 4 + 3 new, `-core-jackson3` 4,
  `-processor` 15, `-loader` 16, `-spring-boot` 14, `-spec-tck` 13).
- Full write-up: `java/README.md`'s updated `terrace-config-core` section and module table.

## `SchemaAssembler`: a `TypeDescriptor` plus a `Dialect` becomes a `Schema` (this session)

The biggest item carried in "Open" across the last several sessions — combining the processor's
`TypeDescriptor` model with a dialect to describe every key a type can carry — is implemented,
unblocking `terrace-config-loader`'s `schema()`/`schemaAt()`.

- **`de.timscho.config.core.descriptor.SchemaAssembler`** is the Java `Schema::describe_at`: it
  walks a `TypeDescriptor`'s fields into a flat `Key` list (a `@Nested` field opens a level rather
  than becoming a key of its own; any container field — including one whose element is itself a
  nested struct via `@Element` — stays one `Structured`-form key, since an environment variable
  cannot address an index inside itself), then derives each key's `env`/`env_file`/`secrets_file`
  spellings and `unreachable` reason by porting the Rust crate's `env_spelling`/
  `secrets_file_name` helpers field for field: an environment name a case-folding,
  separator-splitting reader cannot map back to the same path is `Unnameable`; a name colliding
  with the indirection suffix is `Indirection`; a reserved key's file spellings are cleared,
  exactly as `describe_at` does. `constraint`/`textForm` are derived from the descriptor's own
  type name, `@Range`, and `@Values`/`@ElementValues` spellings.
- **`TerraceLoader.schema()`/`schemaAt(descriptor, root)`** call the assembler with the loader's
  own dialect settings, then append the three loader-read `LoaderVar`s (`config`, `secrets_dir`,
  each `reserve`d key) exactly as `Terrace::schema_at` does — the half no descriptor can see,
  since it belongs to the loader, not the type.
- **Not ported**: filling in each key's observed default from a live instance
  (`Schema::with_defaults_from` needs a `T` to serialise, which is not a static property of a
  descriptor) — every key here is `required` unless its own field is `Optional`, an approximation
  documented on `SchemaAssembler` itself. A future `with_defaults_from` needs either reflection
  over an instance or a value the caller assembles by hand, mirroring the Rust crate's own
  `with_defaults_from_value` escape hatch.
- **A real annotated fixture, not a hand-built `TypeDescriptor`**: `terrace-config-loader`'s new
  tests exercise a compiler-generated `AppConfigDescriptor` (nested struct, a `@Values` enum, a
  `@Secret`, a `@Range`, and a deliberately camelCase field proving `Unnameable`), which needed
  `testCompileOnly`/`testAnnotationProcessor` added on `-processor` — previously only the main
  source set had them.
- **Verified**: `./gradlew clean test` from `java/` — `BUILD SUCCESSFUL`, all 9 modules build;
  `terrace-config-loader` gained `TerraceLoaderSchemaTest` (5/5, new) alongside its unchanged
  `DialectTest` (8/8) and `TerraceLoaderTest` (8/8); every other module's suite unaffected at its
  prior count.
- Full write-up: `java/README.md`'s `terrace-config-core` and `terrace-config-loader` sections.

## The `Contract` producer: `ContractAssembler`, `ProducerIdentity`, and `SpringContractProducer` (this session)

The last remaining open item blocking `terrace-config-spring-boot`'s own producer — assembling a
whole, validated `Contract` rather than only a `Schema` — is implemented, and the Spring module now
ships a working `Contract` producer end to end.

- **`de.timscho.config.core.contract.ContractAssembler`** is the Java `Schema::into_contract` plus
  `ContractBuilder::build`: given a built `Schema`, an `App` and a `Producer`, it renders
  `json_schema` with `JsonSchemaOptions.forContract()` (draft-07, closed, `requirePresent` off —
  the exact options the Rust crate's own `Contract` half uses), derives a declared `ExternalVar`'s
  `constraint` from its `ty`/`values` wherever a caller left it `null` (leaving an already-stated
  one alone, the escape hatch for a domain type this module can't interpret), assembles the
  `Contract`, and runs the already-existing `ContractValidator` — which turned out to need no
  changes at all, since it was already written to accept a whole `Contract` and check all eight
  `spec/v1/FORMAT.md` refusals against it.
- **`ProducerIdentity`** is the one place `Producer.name`/`Producer.version` are decided
  (`terrace-config-java`, and the package's own implementation version, falling back to
  `0.0.0-dev` under a test runner with no manifest) — not settable by a caller, on the Rust
  crate's own `Producer::current` reasoning: a producer that could name itself something else
  could publish a document whose reading rules are another implementation's.
- **`Contract` grew the label/Dockerfile helpers** the Rust crate's own `Contract` has:
  `labels`, `toDockerfileLabels`, `toDockerfileBlock` (wrapped in new `MARKER_BEGIN`/`MARKER_END`
  constants), `checkLabels`, and `verifyLabels` (throwing a new `ContractLabelException` naming
  every missing or mismatched label at once, not just the first). Added directly to `Contract.java`
  as plain instance methods — Lombok's `@Value` only generates the getters/`equals`/`hashCode`/
  `toString`, so a hand-written method on the same class is untouched.
- **`terrace-config-spring-boot` gained `SpringContractProducer.produce`**: wires `SpringDialect`
  into a `de.timscho.config.core.model.Dialect` (same prefix, `_` nesting, `_FILE` suffix), hands a
  `TypeDescriptor` to `SchemaAssembler`, and the resulting `Schema` to `ContractAssembler` alongside
  the caller's `App` — the same composition `terrace-config-loader`'s own `schemaAt` uses, one
  level further. `schema.loader` is always empty for this producer, since Spring's `Binder` (not a
  variable this module reads first) decides what the layers are. Deliberately **not** a starter
  auto-configuration: a service's own configuration type and environment prefix are things only
  that service knows, so a consumer calls `produce` once from its own `@Configuration` class.
- **Verified**: `./gradlew clean test` from `java/` — `BUILD SUCCESSFUL`, all 9 modules build;
  `ContractAssemblerTest` (10/10, new, in `-core`) and `SpringContractProducerTest` (2/2, new, in
  `-spring-boot`, against a real generated `ServiceConfigDescriptor`) pass; every other module's
  suite unaffected at its prior count.

## `terrace-config-loader`'s `explain()`: which layer supplied each key (this session)

The last open item on `terrace-config-loader` itself — a provenance report independent of
`ShadowPolicy`, mirroring the Rust crate's `Terrace::explain` — is implemented and tested.

- **`Layer`, `Fragment`, `Origin`, `Explanation`** are a field-for-field port of
  `rust/src/explain.rs`. `Layer` is a Java `sealed interface` of four records (`Toml`, `Env`,
  `SecretsFile`, `Indirection`) rather than the Rust `enum`, giving the same "a fifth layer is a
  compile error, not a silently incomplete `default`" guarantee. `Origin` keeps the same
  invariant the Rust type keeps by construction — a key is only ever built from a non-empty
  source list, so there is always exactly one `effective()` layer — by making `fromSources` the
  only constructor. `Explanation.toString()` renders the identical report shape: prefix, key
  count, contested count, one header line per layer (TOML/environment/secrets dir/indirection),
  then every key indented under `keys:`, shadowed layers listed beneath the one that won. Like
  the Rust type, an `Explanation` carries no configuration *value* anywhere — only paths and
  variable names — so it's safe to log by construction, not by remembering to redact; an
  unreadable TOML fragment is reported as `Fragment.Unreadable` with no parse detail attached for
  the same reason a parse error can quote the failing line, which can be the credential.
- **`TomlLayers.fragmentKeys`** (new) parses one file on its own and returns every leaf key path
  it declares, or `null` if it won't parse — kept separate from the existing `merged()`, which
  merges across files rather than reporting per file.
- **`FileLayers` gained two package-private accessors**, `secrets()`/`indirections()`, so
  `Explanation.of` can walk each file-backed layer's own key-to-path attribution without
  duplicating `SecretsDir`/`FileSuffixEnv`'s reading logic.
- **`TerraceLoader` refactored around a shared `Layers` record** (dialect, the TOML expansion,
  the file-backed layers, and the raw config/secrets-dir variable state) so `assemble()` (which
  merges it, for `load`) and the new `explain()`/`explain(Map<String, String>)` (which reports on
  it instead) read the environment exactly once and can never disagree about what they saw.
- **Verified**: `./gradlew clean test` from `java/` — `BUILD SUCCESSFUL`, all 9 modules build;
  new `ExplanationTest` passes 7/7 (TOML-only attribution, environment-shadows-TOML, a
  secrets-directory file, `_FILE` indirection, a missing TOML file, an unreadable TOML file with
  no leaked detail, and the full rendered report text); `terrace-config-loader`'s existing
  `DialectTest` (8/8), `TerraceLoaderTest` (8/8) and `TerraceLoaderSchemaTest` (5/5) unaffected;
  every other module's suite unaffected at its prior count.
- Full write-up: `java/README.md`'s `terrace-config-loader` section.

## `load_watched`, the remaining renderings, a Spring starter, `with_defaults_from_value`, and CI (this session)

Every item the "Open" section above had carried across multiple sessions is now addressed: this
was the "keep going until it's all done" pass.

- **`terrace-config-loader` gained `loadWatched(Class)`/`loadWatched(Class, Map)`**, returning a
  new `Loaded<T>` (the bound value plus `Sources`). `FileSuffixEnv.watchPaths()`/
  `FileLayers.watchPaths()` collect the *parent* directory of every file-backed input (the secrets
  directory, each `_FILE` indirection target) rather than the file itself, since a Kubernetes
  volume update replaces a file by renaming a new `..data` directory over the old one — watching
  the old file's inode never fires again. `Sources.differsFrom` compares the pre-binding merged
  map by structural equality with `Double`/`Float` compared by raw bits, so a `NaN` in the
  configuration compares equal to itself instead of making every reload look like a change. The
  fingerprint is deliberately never exposed by `Sources.toString()` — it is every configuration
  value, secrets included.
- **`terrace-config-core`'s three remaining renderings are built**: `Schema.toMarkdown()`/
  `toMarkdownWith(columns)`/`toMarkdownLoader()`/`toMarkdownKeys(columns)` (a `Column` enum of 13
  columns, `DEFAULT_COLUMNS` narrower than the full set, GitHub-flavoured tables with `|`/`\`
  escaping); `Schema.toTomlExample()`/`toTomlExampleWith(options)` (a commented `config.toml` where
  everything with a default is commented out — a commented key and a deleted key mean the same
  thing to the loader — secrets always redacted to a placeholder regardless of their real default,
  and every literal rendered as real TOML, verified in a test by actually parsing the generated
  file with `tomlj`); and `Schema.withDefaultsFromValue(Map)` (the Java analogue of
  `Schema::with_defaults_from_value` — `-core` has no Jackson runtime of its own, so it takes an
  already-nested `Map` rather than serialising a live instance itself).
- **`terrace-config-spring-boot` gained a starter auto-configuration**:
  `TerraceContractProperties` (`terrace.contract.type`/`env-prefix`/`app-name`/`app-version`) and
  `TerraceContractAutoConfiguration` (`@AutoConfiguration`, gated on `terrace.contract.type` being
  set, looks up `<Type>Descriptor.DESCRIPTOR` by reflection and calls `SpringContractProducer`),
  registered via `META-INF/spring/...AutoConfiguration.imports`. A service with the type nested
  inside another class still has to call `produce` by hand, since reconstructing the processor's
  own `$`-joined naming from a bare class name is not attempted.
- **CI now builds and tests `java/`.** `ci.yml` gained a `changes` job (`dorny/paths-filter`) and a
  `java` job (`actions/setup-java` + `gradle/actions/setup-gradle`, `./gradlew clean test`) gated
  on `java/**` having changed, wired into the `ci` gate's `needs:`.
- **`release-please-config.json`/`.release-please-manifest.json` gained a `java` package**
  (`release-type: simple`, `include-component-in-tag: true` so its tags don't collide with
  `rust`'s untagged convention, `extra-files: ["java/build.gradle.kts"]`), a version marker
  (`// x-release-please-version`) added next to `version = "0.1.0-SNAPSHOT"`, and a new
  `java/CHANGELOG.md`.
- **The upstream `TimSchoenle/actions` inputs were finally confirmed — and two real bugs were
  found and fixed.** The earlier 404s were because the action files are `action.yaml`, not
  `.yml` (a spelling this session didn't try before). With that fixed, the GitHub API confirms:
  `rust/{clippy,test,cargo-check}` take `source_directory`, not `working-directory` (`ci.yml` had
  been passing the wrong input name since the crate was moved under `rust/` — silently ignored
  rather than erroring, so the working directory was never actually being set); and
  `readme-variables` takes `docs-dir`, not `docs-directory` (same silent-ignore bug in `docs.yml`).
  Both are fixed now. Every other input already in use (`manifest`, `render-template`'s
  `template`/`output`/`variables`/`check`, `render-template-and-commit`'s `token`/
  `commit_message`, `commit-changes`'s `file_pattern`) was checked against the real `action.yaml`
  at the exact pinned commit and matches.
- **`Key` gained `@Builder(toBuilder = true)`**, needed by `Defaults.withDefaultsFromValue` to
  produce a modified copy of a key without hand-writing every field again.
- **Verified**: `./gradlew clean test` from `java/` — `BUILD SUCCESSFUL`, all modules build;
  new suites all green (`LoadWatchedTest` 7/7, `MarkdownRendererTest` 5/5, `TomlExampleRendererTest`
  7/7, `DefaultsTest` 7/7, `TerraceContractAutoConfigurationTest` 5/5); every pre-existing suite
  unaffected at its prior count.

## Open

- **`terrace-config-core`'s `dockerfile`/label renderings were already built** in an earlier
  session (`Contract.toDockerfileBlock`/`labels`); nothing further is deferred there.
- **`SchemaAssembler` still derives `Key.required` from `Optional`-wrapping alone**, not a live
  default value — this remains a reasonable approximation, since Rust's own `required` flag is
  also set by the type shape, not by `with_defaults_from`; that method only ever fills in
  `Key.default`, which the Java side can now do too via `Schema.withDefaultsFromValue`. No further
  action needed here.
- **The Spring starter auto-configuration only resolves a top-level annotated type** by
  reflection; a nested annotated type still needs `SpringContractProducer.produce` called by hand.
- **Publishing** (Maven Central / GitHub Packages coordinates, signing, a `publish` CI job) is not
  set up; the `release-please` wiring added this session only tracks a version number and
  changelog, it does not yet cut an artifact.
- **The two open dialect questions** (a `structured` key's single-variable spelling under a
  binder that also produces indexed names; whether `text_form` needs a value for a fixed-point
  type) are unchanged from prior sessions — `terrace-config-spring-boot`'s binder-based approach
  still hasn't needed to answer either in practice.
