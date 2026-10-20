<!--
Generated from .github/templates/java-README.md.hbs — edit that file, not this one.

CI renders it on every pull request and commits the result back to the branch, the same way it
does for rust/README.md and README.md. A push to `main` whose java/README.md does not match its
template fails the `readme` job in .github/workflows/docs.yml.

The payload is the one rust/README.md renders from: TimSchoenle/actions/actions/common/readme-variables
reads rust/Cargo.toml and walks rust/docs/, merged over the output of
`bash .github/scripts/readme-variables.sh`. This template reads only `repo`.

Nothing in this comment may contain a mustache that is not a real reference.
-->

# terrace-config (Java)

The Java implementations of the configuration contract at [`spec/`](../spec/README.md).

[![Release](https://img.shields.io/github/v/release/TimSchoenle/terrace-config?sort=semver)](https://github.com/TimSchoenle/terrace-config/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/TimSchoenle/terrace-config/ci.yml?branch=main&label=ci)](https://github.com/TimSchoenle/terrace-config/actions/workflows/ci.yml)
[![Licence](https://img.shields.io/github/license/TimSchoenle/terrace-config)](../LICENSE)

## What this is

Two implementations of [`spec/`](../spec/README.md)'s contract, built on one shared pipeline: an
annotation processor reads `@TerraceConfig` types at compile time and generates the descriptors
[`terrace-config-loader`](#terrace-config-loader) and
[`terrace-config-spring-boot`](#terrace-config-spring-boot) both assemble into a `Contract`.
Neither hand-writes what a document should look like — `terrace-config-spec-tck` checks both
against the same conformance corpus the Rust reference implementation authors, so a renamed field
shows up as a failing test rather than a document nobody noticed had drifted.

## Modules

| Module | What it is | Status |
|---|---|---|
| `terrace-config-annotations` | `@TerraceConfig` and the rest of the vocabulary a service's own types carry. Depends on nothing but the JDK. | **implemented** |
| `terrace-config-core` | The envelope model, the twelve build-time refusals, and the JSON Schema half a contract embeds under `json_schema`. Knows about documents, not binders, and about neither Jackson major — see `-core-jackson2`/`-core-jackson3` for the `json` codecs. | **implemented** — validation, a Jackson-version-agnostic model, `Schema.toJsonSchemaWith`, and what the build-time producers share (`ProducerArguments`, `ProducerCommandLine`, `ConfigurationType`). Every other rendering — `markdown`, `toml`, the standalone `json-schema`, `labels`, `dockerfile` — is `terrace-contract render`'s alone (`docs/contract-cli-plan.md` §6) |
| `terrace-config-core-jackson2` | The byte-stable `json` codec for the `-core` model, built on Jackson 2.x. | **implemented** |
| `terrace-config-core-jackson3` | The same codec, built on Jackson 3.x instead. | **implemented** |
| `terrace-config-processor` | The JSR-269 annotation processor generating descriptors from annotated types. A field's key path follows its `@JsonProperty` rename when present, matching whichever binder actually reads that name (`FieldResolver.resolve`), the same way it already follows one for an enum constant. | **implemented** |
| `terrace-config-loader` | The vanilla five-layer loader. `producer.name` is `terrace-config-java`, `producer.loader` `terrace-java`, target **tier 2**. | **implemented** — layers, dialect, shadow policy, `schema()`, `explain()`, watched reload, and `ContractMain`, which emits the contract at build time |
| `terrace-config-spring-boot` | The Spring Boot side, over Spring's own `Binder`. `producer.loader` is `spring-boot`, target **tier 1**. | **implemented** — the `_FILE` indirection `EnvironmentPostProcessor`, the `_`-vs-`__` dialect divergence, `SpringContractProducer` assembling a full validated `Contract` from a `TypeDescriptor`, and `SpringContractMain`, which emits it at build time |
| `terrace-config-spec-tck` | The meta-schema validator and tier comparator, checked against the stored corpus in `spec/v1/conformance/`, plus `JavaConformanceTest`: real `@TerraceConfig` fixtures for every named case (`minimal`, `full-surface`, `unnameable-key`, `required-entries`, `entry-names`, `element-patterns`) rendered through the real loader and Jackson 2 codec, checked at tier 1 (meta-schema), tier 2 (dialect spellings against the shared spec corpus) and tier 3 (byte-for-byte against this module's own `conformance-java/` golden corpus — regenerate with `./gradlew :terrace-config-spec-tck:blessJavaConformance`). `JacksonParityTest` proves the Jackson 2 and Jackson 3 codecs render the same bytes. `TerraceContractConformTest` runs the real `terrace-contract conform` over every document the two producers emit for those fixtures (loader at tier 2, Spring at tier 1), the stored Java goldens, and both examples' committed `contract.json`. | **implemented** — validates every stored case against `contract.schema.json` and its schema-only sub-schema; compares two documents at tier 1/2/3; `terrace-config-java` tier 3 against itself and tier 2 against the spec corpus; conformed by `terrace-contract` |
| `terrace-config-gradle-plugin` | The `de.timscho.terrace-config` Gradle plugin: emits a service's contract from its types at build time, stamps and conforms it with `terrace-contract`, and fails `check` when the committed copy drifts. See [its section](#terrace-config-gradle-plugin). | **implemented** |
| `terrace-config-example-service` | A worked example: a small "orders" service loading its configuration through `-loader`, mirroring `rust/examples/service/` field for field. `OrdersServiceConfigTest` exercises the example's own `Config` class through `TerraceLoader`, so it stays correct by a real assertion rather than by compiling alone — run it with `./gradlew :terrace-config-example-service:run`. `Config` is also `@TerraceConfig`; the `de.timscho.terrace-config` plugin emits its contract at build time, and `terraceContractCheck` holds the committed `contract.json` to it on every `check`. Its `Dockerfile` builds an image carrying the stamped contract. | **implemented** |
| `terrace-config-example-spring-service` | The same "orders" service, this time configured through `-spring-boot` instead of `-loader`. `OrdersPropertiesBindingTest` exercises `OrdersProperties` through Spring's own `Binder` via `ApplicationContextRunner`; `OrdersServiceFileIndirectionIntegrationTest` boots the real `SpringApplication` — the one thing a context runner cannot do — to prove `_FILE` indirection actually reaches a bound bean, including the `_`-vs-`__` divergence's own failure mode. Run it with `./gradlew :terrace-config-example-spring-service:run`. The plugin gives it the same checked `contract.json` and `Dockerfile` as the loader example, this time through `SpringContractMain`. | **implemented** |

## Building and testing

```bash
cd java
./gradlew test
```

The wrapper (`gradlew`/`gradlew.bat`) is committed, so no local Gradle install is needed — it
pulls the pinned version (9.7.1, which supports Java 25) on first run. Module
versions live in one place, `gradle/libs.versions.toml`, the same role `<dependencyManagement>`
played before. Shared build logic (the Java toolchain, test dependencies, Lombok wiring) lives in
`buildSrc`'s convention plugins rather than being repeated per module — see "buildSrc" below.

`terrace-config-spec-tck`, `terrace-config-core`, `terrace-config-core-jackson2` and
`terrace-config-core-jackson3` are, so far, the modules with anything to test: all read
`../spec/v1/conformance/*/contract.json` directly, so none needs a producer or any other module to
exercise for real. Each codec module's own corpus test (`ContractCorpusRoundTripTest`)
deserialises every stored case into the shared model and re-serialises it, asserting the result is
byte-identical to the stored file — the strongest check the model has, run once per Jackson major
against the exact same model classes, and the one the plan calls for before any producer exists.
`terrace-config-processor`'s own suite (`TerraceConfigProcessorTest`) runs the processor through
`compile-testing` against small in-memory sources rather than the corpus — it has no producer or
Json document of its own to check against yet, only whether a given annotated type's shape is
resolved or correctly refused. `terrace-config-loader`'s own suite exercises real files in a JUnit
`@TempDir` (a TOML fragment, a secrets directory, `_FILE` indirection) rather than the corpus,
since it has no `Contract`-shaped output of its own yet — see below. `terrace-config-spring-boot`'s
own suite uses a `MockEnvironment` from `spring-test` rather than a real `SpringApplication`, since
`EnvironmentPostProcessor`s run before there is a context to boot — see below.
`terrace-config-example-service`'s own suite is the same shape as `-loader`'s, against the exact
`Config` class its `OrdersService.main` boots with, rather than a type redeclared for the test.
`terrace-config-example-spring-service`'s suite splits in two: `OrdersPropertiesBindingTest` binds
`OrdersProperties` through `ApplicationContextRunner`, a lightweight context, while
`OrdersServiceFileIndirectionIntegrationTest` boots a real `SpringApplication` — swapping in a
fake `SystemEnvironmentPropertySource` before `run()` rather than touching this JVM's own
environment — because `_FILE` indirection is an `EnvironmentPostProcessor`, which a context runner
never invokes.

Both example modules apply the [`de.timscho.terrace-config`](#terrace-config-gradle-plugin)
plugin, whose `terraceContractCheck` (part of `check`) emits a fresh contract from the module's own
annotated type and compares it, byte for byte, against the `contract.json` checked into that
module's directory — the same "regenerated and diffed, never trusted merely because it once
matched" guarantee `cargo test`'s `the_checked_in_contract_matches_what_the_types_render_today`
gives the Rust crate's own `examples/service/`. Regenerate either with
`./gradlew :terrace-config-example-service:terraceContractUpdate`. A key whose Java name changes
case in a way the environment fold cannot reverse — `bindAddr`, say — renders with `"env": null`
and `"unreachable": "unnameable"` rather than a spelling nothing would actually respond to; see
`terrace-config-example-service`'s own `Config` (which works around it with `@JsonProperty`,
now that `terrace-config-processor` reads the same rename `-loader`'s binder does) against
`terrace-config-example-spring-service`'s `OrdersProperties` (which does not need to, and so
leaves `bindAddr`, `database.maxConnections` and `logLevel` genuinely unreachable in the contract
even though Spring's own relaxed binding happens to tolerate them).

`TerraceContractConformTest` (in `-spec-tck`), the plugin's functional tests and each example's
`terraceContract` task run the real `terrace-contract` binary. The tests find it at
`TERRACE_CONTRACT_BIN`, else `../cli/target/release/terrace-contract`, then
`../cli/target/debug/terrace-contract` (what `cargo build` in `cli/` leaves), else on `PATH`, and
hand it to the builds they run through `TERRACE_CONTRACT_BIN`. The examples' `terraceContract`
uses the plugin's own lookup, which is `TERRACE_CONTRACT_BIN` or `PATH` and nothing else, so it
needs the variable set. Without a binary, the tests **fail when `CI=true`** and are skipped
through a JUnit assumption otherwise, so CI cannot go green by skipping them while a contributor
without a Rust toolchain can still run the rest. CI builds the binary from the same commit:

```bash
(cd ../cli && cargo build --release)
TERRACE_CONTRACT_BIN="$PWD/../cli/target/release/terrace-contract" ./gradlew check terraceContract
```

## Conventions

No streams (loops), Lombok for the model classes, minimal dependencies, `@NullMarked` packages
with jspecify `@Nullable` for genuine absence (see "Null-safety" below), and
deterministic-by-construction rendering — ordered collections and explicit property order rather
than relying on incidental iteration order. Comments explain why, not what.
`terrace-config-spec-tck` and `terrace-config-core` already follow all of them.

## `terrace-config-core`

The document model — `Contract` and everything it's built from (`de.timscho.config.core.model`)
— and the twelve build-time refusals (`de.timscho.config.core.refusal`), all mirroring
[`spec/v1/contract.schema.json`](../spec/v1/contract.schema.json) field for field and property for
property. Deliberately builds no `ObjectMapper` of either Jackson major itself — see
`terrace-config-core-jackson2`/`-jackson3` below for the two codecs that do, and "Dual Jackson 2 /
3 support" for why the split sits there and not here.

- **The model** is one Lombok `@Value @Builder @Jacksonized` class per meta-schema object
  (`Producer`, `App`, `Schema`, `Dialect`, `LoaderVar`, `Key`, `External`, `ExternalVar`), four
  enums for the closed vocabularies (`LoaderRole`, `TextForm`, `UnreachableReason`,
  `ExternalUnknownPolicy`), and one open, order-preserving `JsonSchemaDocument` for the
  `json_schema` half, which has no closed shape by design. Every class carries an explicit
  `@JsonPropertyOrder` and per-field `@JsonProperty` (rather than a class-level `@JsonNaming`,
  which only Jackson 2's `databind` package understands) and, on `Key`/`ExternalVar`, explicit
  `@JsonInclude` per field — the meta-schema's `required` list and its "open at every level"
  properties list don't coincide (e.g. `constraint` is optional, `ty` is required-but-nullable),
  so the inclusion rule has to be decided field by field rather than once per class.
- **`ContractValidator.validate(Contract)`** runs `spec/v1/FORMAT.md`'s "What a producer MUST
  refuse" — all twelve ways a contract could quietly stop being one — and throws the first
  violation as its own `ContractRefusalException` subclass. It lives here, once, rather than in
  each of `-loader` and `-spring-boot`, because both build the same `Contract` shape and must
  refuse the same twelve things.
- **`ContractValidatorTest`** has one test per refusal (plus one proving a well-formed contract
  passes all twelve), each built with a minimal `Contract` rather than a full corpus case, to keep
  the refusal under test isolated from the other eleven. The byte-stable round-trip test lives in
  each codec module instead (see below), since it needs an actual `ObjectMapper` to run.
- **`Schema.refine(path, Refinement)` / `refineWith(at, Refine)`** (`de.timscho.config.core.schema`)
  publish a constraint a type cannot state — a map's required entries, via
  `Refinement.requiredEntries(…)`, and the pattern its entry names must match, via
  `Refinement.entryNames(…)` — with the Rust crate's rules and messages: an unknown path, a key that
  is not an open map, an entry name some layer could not spell, a pattern outside the portable
  subset (`PortablePattern`), or a required entry the pattern rejects is a `RefinementException`; a
  default the refinement rejects makes the key required with no default, whichever of `refine` and
  `withDefaultsFromValue` ran first. An entry-name pattern raises the schema to
  `schema_version: 3`. `Refinement.holds(Condition…)` relates a struct's fields — exactly one of,
  when-then, present, empty, equal, above, matches — published as a described `allOf` member at
  `schema_version: 4`, with the Rust crate's encodings and sentences. `Refinement.pattern(…)` and
  `Refinement.nonBlank()` hold a string to a pattern, and a path continues inside a key's constraint — `*` for an element, a field name for a
  field — wherever the constraint describes one. (Maps publish no element schema from this producer
  yet, so the positions inside one are reachable only on a constraint built by hand.) A library
  implements `Refine` with paths relative to wherever a host mounts it.
- **`Schema.toJsonSchemaWith(JsonSchemaOptions)`** (`de.timscho.config.core.schema`) renders a
  `Schema` as the JSON Schema half a `Contract` embeds under `json_schema` — a straight port of
  the Rust crate's `schema::json_schema` module. `Node` groups the flat, dotted `Key.path` list
  into the nested tree a `properties` object needs; `leaf` copies each key's own `constraint` in
  rather than re-deriving it, adds `description`/`default`/`writeOnly`;
  `JsonSchemaOptions.forContract()` is draft-07, closed, `requirePresent` off — the exact options
  the Rust crate embeds with, and the ones `ContractAssembler` calls. Every map returned is a
  `TreeMap`, so its iteration order is already alphabetical, the same order `serde_json`'s own
  (non-`preserve_order`) map produces. The standalone document an editor validates against is
  `terrace-contract render --format json-schema`, read off the emitted contract.
- **`JsonSchemaRendererCorpusTest`** (in `-core-jackson2`, since it needs a real `ObjectMapper` to
  parse the corpus) re-renders each stored case's `schema` and asserts it structurally equals that
  same case's stored `json_schema` field — proving the port against real output rather than only
  hand-built fixtures, with no Java producer of its own needed to do it.
- **`de.timscho.config.core.descriptor.SchemaAssembler`** combines a dialect-agnostic
  `TypeDescriptor` (from `terrace-config-processor`) with a `Dialect` into a full `Schema` — the
  Java equivalent of `Schema::describe_at`, and what unblocked `-loader`'s `schema()`/`schemaAt()`
  (see below). It walks a type's fields into a flat `Key` list (a `@Nested` field opens a level
  rather than becoming a key of its own; a container field — `List`/`Set`/`Map`, nested-struct
  elements included — stays one `Structured`-form key, since an environment variable cannot address
  an index inside itself). That key's `constraint` carries the shape of one element, as
  `rust_type::interpret_with` does: `items` for a `List`/`Set`, `additionalProperties` for a `Map`
  — a scalar element's own type, or an `@Element` struct's keys as nested `properties` with
  `required`, closed (`additionalProperties: false`) exactly where the struct carries
  `@JsonIgnoreProperties(ignoreUnknown = false)`, and composed as deep as element structs hold
  containers of their own (`JsonSchemaRenderer.elementObject`, the port of `element_object`). It
  also ports `env_spelling`/`secrets_file_name` field for field: an
  environment name that a case-folding, separator-splitting reader can't map back to the same path
  is `Unreachable.Unnameable`; a name colliding with the indirection suffix is
  `Unreachable.Indirection`; a reserved key's file spellings are cleared, exactly as
  `describe_at` does. `Key.required` is `false` whenever the field is `Optional`-wrapped or
  `KeyDescriptor.hasDefault()` — `FieldResolver` reads the latter off the field's own declaration
  via the compiler's `Trees` API (whether it carries an initializer at all, e.g. `private String
  port = "8080";` versus `private String port;`), the Java analogue of the Rust derive macro's own
  syntactic `!(opts.has_serde_default || container.field_default || is_option(ty))` check — neither
  language needs a live instance to know a key is optional. Filling in what the default actually
  *is* still needs one; see `Schema.withDefaultsFromValue` below for the piece of `Schema::
  with_defaults_from_value` that is ported, and `ContractMain`/`SpringContractMain` (below) for
  the callers that run it against a default-constructed instance.
- **Every other rendering is `terrace-contract render`'s.** The Markdown tables, the commented
  `config.toml`, the standalone JSON Schema, the image labels and the Dockerfile marker block are
  all read off the emitted document by `terrace-contract render --format
  markdown|toml|json-schema|labels|dockerfile`, and an image, or a Dockerfile's committed label
  block, is checked against its contract by `terrace-contract image verify`
  (`docs/contract-cli-plan.md` §5). This module carried its own copies of those renderers once;
  they were removed (§6), since a second implementation of a rendering can only drift from the
  first.
- **`Schema.withDefaultsFromValue(Map<String, Object>)`** is the Java piece of
  `Schema::with_defaults_from_value` that is portable without a Jackson runtime in `-core`: given
  an already-nested map (e.g. `objectMapper.convertValue(instance, Map.class)` from a caller that
  does have one), it fills in each non-required key's `defaultText`/`defaultValue` from the
  observed value, redacting a `Key.secret` key to `<redacted>` after rendering rather than before
  (an unset secret is not a secret worth hiding). A required key is left untouched by definition.
- **A real Lombok/Gradle interaction, found while adding this**: `Node`'s package-private-by-default
  fields compiled as *private* and broke access from `JsonSchemaRenderer` in the same package,
  because `java/lombok.config`'s `lombok.fieldDefaults.defaultPrivate = true` applies to every
  field with no explicit access modifier in any Lombok-processed module, not only fields on
  Lombok-annotated classes. Fixed by giving `Node`'s fields an explicit `public` modifier — harmless
  since `Node` itself is package-private.
- **`de.timscho.config.core.contract.ContractAssembler`** combines a built `Schema` with an `App`
  and a `Producer` into a full, validated `Contract` — the Java equivalent of the Rust crate's
  `Schema::into_contract` followed by `ContractBuilder::build`. It renders `json_schema` via
  `JsonSchemaOptions.forContract()`, derives a declared `ExternalVar`'s `constraint` from its
  `ty`/`values` wherever a caller left it unset (the escape hatch for a domain type is stating the
  constraint outright, exactly as the Rust crate's own builder leaves it alone once set), and
  finally runs the already-existing `ContractValidator` — written once here so `-loader` and
  `-spring-boot` do not each reimplement assembly. `ProducerIdentity` supplies the one `Producer`
  identity this module is allowed to publish (`name`/`version` are not caller-settable, on the
  same reasoning the Rust crate's `Producer::current` gives).
- **`ProducerArguments`, `ProducerCommandLine` and `ConfigurationType`**
  (`de.timscho.config.core.contract`) are what `-loader`'s `ContractMain` and `-spring-boot`'s
  `SpringContractMain` share: a `--name value` parser with no dependency (the entry points run on a
  service's own classpath, where every library they brought would be one more version to
  reconcile), `--out`/`--help` and the exit statuses `terrace-contract` itself uses (`0` written,
  `1` refused, `2` usage or classpath error), and the generated descriptor and default-constructed
  instance of a type named by its binary name. `ConfigurationType` loads the type without
  initialising it; the descriptor is always the binary name plus `Descriptor`, nested types
  included (see `DescriptorNaming` below).

## `terrace-config-core-jackson2` / `terrace-config-core-jackson3`

Two sibling modules, each providing `de.timscho.config.core.io.ContractCodec` — the byte-stable
`json` reader/writer for `-core`'s `Contract` — built on one Jackson major apiece
(`com.fasterxml.jackson.databind` / `tools.jackson.databind`). Same package, same class name,
same public methods (`read`/`write`), deliberately: a consumer picks one module as a dependency
based on which Jackson major it already has, and the call sites look identical either way. Neither
module depends on the other; both depend only on `-core` and their own Jackson major.

- **`ContractCodec`** is the only place an `ObjectMapper` is constructed, once, statically. Its
  `CompactEmptyContainerPrettyPrinter` exists for one reason: Jackson's own `DefaultPrettyPrinter`
  renders an empty array as `[ ]` and a field separator as `"key" : value`, and the corpus (like
  every hand-authored `contract.json`) uses `[]` and `"key": value`. `spec/v1/FORMAT.md`'s
  "Publication" section defines the document as byte-stable, so this is not cosmetic — it's what
  makes `ContractCorpusRoundTripTest` (below) possible at all. The Jackson 3 copy differs from the
  Jackson 2 one only where the API forced it to: `writeObjectFieldValueSeparator` was renamed
  `writeObjectNameValueSeparator`, `ObjectMapper` is built once, immutably, via `JsonMapper.builder()`
  rather than mutated after construction, and its streaming methods throw the unchecked
  `JacksonException` instead of `IOException`.
- **`ContractCorpusRoundTripTest`**, one copy per module, deserialises every stored
  `spec/v1/conformance/*/contract.json` into `Contract` and re-serialises it, asserting the result
  is byte-identical to the file on disk — proving the exact same `-core` model classes really do
  work under both Jackson majors, not just the one exercised first.

## buildSrc

`java/buildSrc/` is its own standalone Gradle build (a Gradle convention, not something wired up
by hand) that compiles three *convention plugins* under
`buildSrc/src/main/kotlin/terrace-config.*-conventions.gradle.kts` and makes them available, by
ID, to every module below it:

- **`terrace-config.java-conventions`** — the Java 25 toolchain, UTF-8 source/Javadoc encoding,
  the JUnit 5 + AssertJ test dependencies, and the `org.jetbrains.annotations` `compileOnly`
  dependency (see "Null-safety" below) every module gets. Applied by every module, either
  directly (`terrace-config-annotations`, `terrace-config-spec-tck`,
  `terrace-config-gradle-plugin`) or transitively through the plugin below.
- **`terrace-config.lombok-conventions`** — the above, plus `io.freefair.lombok` pre-configured
  with the version from `gradle/libs.versions.toml`. Applied by `terrace-config-core`,
  `terrace-config-processor`, `terrace-config-loader` and `terrace-config-spring-boot`.
- **`terrace-config.publish-conventions`** — `com.vanniktech.maven.publish` pre-configured with
  the licence/developer/SCM POM metadata common to every published artefact, plus javadoc and
  sources jars. Applied by the eight modules with a public artefact — see "Publishing to JitPack"
  below for which those are, and why Maven Central itself isn't wired up (yet). A module that
  applies `java-gradle-plugin` before it (only `terrace-config-gradle-plugin`) is published as a
  Gradle plugin, with the plugin marker publication, instead of as a library.

buildSrc also compiles `terrace-config-gradle-plugin`'s own sources a second time and registers
the same `de.timscho.terrace-config` id, which is how the two example modules apply the plugin by
id in this same build: a build cannot apply a plugin one of its own subprojects publishes, and
buildSrc's classes are on every module's build script classpath. One set of sources, so the copy
the examples run and the copy JitPack publishes cannot differ.

This replaces what used to be a root `java/build.gradle.kts` `subprojects { ... }` block: the same
settings, but expressed as plugins a module opts into by ID rather than configuration silently
applied to every project from outside. The root `java/build.gradle.kts` itself is now just the
shared `group`/`version` (`allprojects`), since there's nothing else left to centralize there.
`buildSrc/settings.gradle.kts` points buildSrc's own version catalog at the same
`gradle/libs.versions.toml` file the modules use, so there's still exactly one file to bump a
version in — but note that Gradle's type-safe `libs.xxx` accessors don't work *inside* a
precompiled script plugin (a documented limitation: the plugin might end up applied to a project
with a differently-shaped catalog), so the convention plugins above use the lower-level,
string-keyed `VersionCatalogsExtension` lookup instead (`libs.findLibrary("...")`,
`libs.findVersion("...")`); ordinary module `build.gradle.kts` files keep using the type-safe
accessors as before.

## Publishing to JitPack

Eight of the eleven modules carry `terrace-config.publish-conventions` and are published:
`terrace-config-annotations`, `terrace-config-core`, `terrace-config-core-jackson2`,
`terrace-config-core-jackson3`, `terrace-config-processor`, `terrace-config-loader`,
`terrace-config-spring-boot`, `terrace-config-gradle-plugin`. All eight share the one version in
`build.gradle.kts`, so release-please's `java` package releases the plugin with the libraries.
`terrace-config-spec-tck` and the two `-example-*` modules
deliberately do not — the first is a test-only tool against `spec/v1/conformance/`, and the
other two are worked examples meant to be read, not depended on.

Coordinates need nothing set per module: `group` (`de.timscho`) and `version` come from the
`allprojects {}` block in the root `build.gradle.kts`, and the plugin's default `artifactId` is
the Gradle project name, which already matches. A module states only its own `pom.name`/
`pom.description`, taken from the `description` property every `build.gradle.kts` already
declares — everything else common to a release (licence, developer, SCM URL) lives once in
`terrace-config.publish-conventions`.

Maven Central itself is not wired up right now — no Sonatype account, no GPG key, no signing —
by choice, not by omission: [JitPack](https://jitpack.io) needs none of that. It builds directly
from a pushed tag, on request, the first time someone resolves it, and caches the result — no
release workflow to run at all, since there's no separate publish step to trigger; the tag itself
is the release. `terrace-config.publish-conventions.gradle.kts`'s own comment says what to add
back (`publishToMavenCentral`, `signAllPublications`, a `release-java.yml`) if Central publishing
is wanted later.

`jitpack.yml`, at the repository root (JitPack always looks there, not under `java/`), tells it
the project's actual Gradle root isn't the repository root and to skip the test suite — this
repository's own CI (`ci.yml`) already runs and gates on it; JitPack is building on demand for
whoever is waiting on the resolution that triggered it, not re-running this repository's checks:

```yaml
jdk:
  - openjdk21
install:
  - cd java && ./gradlew publishToMavenLocal -x test --console=plain
```

`publishToMavenLocal`: JitPack's whole model is copying whatever lands in `~/.m2/repository` into
what it serves back to a consumer — confirmed by running the exact command above locally, which
lands working jars, sources jars, javadoc jars and POMs in `~/.m2` for all eight modules, no
signing or credentials needed anywhere in the path. The Java 25 toolchain itself is unaffected by
whatever JDK actually runs the wrapper on JitPack's build image — see
`org.gradle.toolchains.foojay-resolver-convention` in `settings.gradle.kts`, added for exactly
this: it fetches a matching JDK 25 itself rather than requiring the host to already have one.

A consumer depends on JitPack the same way as any other repository, against a pushed tag or
commit — a released version like `v0.1.0` once release-please has cut one, or a branch name /
short commit SHA for something unreleased:

```kotlin
repositories {
    maven { url = uri("https://jitpack.io") }
}

dependencies {
    // one module — group is `com.github.<user>.<repo>`, artifact is the Gradle module name
    implementation("com.github.TimSchoenle.terrace-config:terrace-config-loader:<tag>")
    // every published module at once — group is `com.github.<user>`, artifact is the repo name
    implementation("com.github.TimSchoenle:terrace-config:<tag>")
}
```

Only the eight modules `terrace-config.publish-conventions` applies are resolvable this way — the
same eight listed at the top of this section, for the same reason.

The Gradle plugin resolves from JitPack too. JitPack serves every artefact under its own
`com.github.TimSchoenle.terrace-config` group, not under the `de.timscho.terrace-config` group a
plugin marker is looked up by, so a consumer's `settings.gradle.kts` maps the id to the module:

```kotlin
pluginManagement {
    repositories {
        gradlePluginPortal()
        maven { url = uri("https://jitpack.io") }
    }
    resolutionStrategy {
        eachPlugin {
            if (requested.id.id == "de.timscho.terrace-config") {
                useModule("com.github.TimSchoenle.terrace-config:terrace-config-gradle-plugin:${requested.version}")
            }
        }
    }
}
```

and applies it with `id("de.timscho.terrace-config") version "<tag>"`.

## Formatting

Every module gets [`com.diffplug.spotless`](https://plugins.gradle.org/plugin/com.diffplug.spotless)
from `terrace-config.java-conventions` — the same "one place, every module" wiring as the Java
toolchain and test dependencies above, not something left to each contributor's own IDE settings.

- **`palantirJavaFormat`**, not `googleJavaFormat`: it keeps 4-space indentation and a 120-column
  width, what every file under `java/` already used by hand, where google-java-format would force
  2-space/100-column and reformat the whole tree away from its existing style. The version comes
  from `gradle/libs.versions.toml`'s `palantir-java-format` entry, resolved by Spotless itself at
  format time rather than put on any module's own classpath. Matches
  `.idea/palantir-java-format.xml`, the IntelliJ side of the same formatter.
- **`formatAnnotations()`** keeps a `TYPE_USE` annotation (jspecify's `@Nullable`, see
  "Null-safety" below) inline on the field/parameter it types, rather than pushed onto its own line
  the way a declaration annotation like `@Override` would be — the one case a plain
  `palantirJavaFormat()` call gets wrong on its own.
- **`importOrder("java", "javax", "", "de.timscho")`**: the JDK first, then every third-party
  import together alphabetically (Jackson, Lombok, jspecify, JetBrains, ...), then this codebase's
  own `de.timscho.*` packages last — one canonical order instead of the ad-hoc per-file grouping
  that predates this setup. `removeUnusedImports()` runs alongside it.
- **Enforced in CI** (`./gradlew spotlessCheck`, in the same step as `test` in `.github/workflows/ci.yml`'s
  `java` job) rather than only applied locally. Run `./gradlew spotlessApply` to fix violations —
  Spotless reports the exact diff `spotlessCheck` would otherwise fail on.

## Lombok

The [`io.freefair.lombok`](https://plugins.gradle.org/plugin/io.freefair.lombok) Gradle plugin,
version pinned once in `buildSrc/build.gradle.kts`, is wired up by the
`terrace-config.lombok-conventions` plugin (see "buildSrc" above) rather than applied by hand in
each module — so a module opts in with a single `id("terrace-config.lombok-conventions")` line,
no repeated `lombok { version.set(...) }` block. It's applied (transitively) to every module that
has, or will have, bean-shaped classes: `terrace-config-core`, `terrace-config-loader` and
`terrace-config-spring-boot`. Three modules deliberately opt out, each for its own reason
documented in its `build.gradle.kts`: `terrace-config-annotations` (annotation types can't carry
Lombok annotations, and the module's whole point is "nothing but the JDK"), `terrace-config-processor`
(its own code is plain `javax.annotation.processing`/`javax.lang.model` logic, and the descriptors
it emits reference `-core`'s plain Java records, never a Lombok-annotated class of its own), and
`terrace-config-spec-tck` (its classes predate the plugin and aren't bean-shaped; new classes there
can still opt in).

The plugin wires `compileOnly`/`annotationProcessor` for both main and test sources automatically
and points `javadoc` at delomboked sources, so generated Javadoc shows real method signatures
instead of `@Data`/`@Builder` annotations. Shared behaviour — `@Generated` on generated methods
(so coverage tools skip them), `callSuper = call` on `@EqualsAndHashCode`/`@ToString`, and a
`private`-by-default field visibility — lives in one place, the root `lombok.config`, which Lombok
picks up by walking up from each source file's directory (`config.stopBubbling = true` stops that
walk at `java/`, so the rest of the repository is unaffected). IntelliJ/RustRover need the Lombok
plugin installed and annotation processing enabled to display generated members while editing;
both are already true for anyone who has opened `terrace-config-core` in this session.

## Null-safety

Every library package under `src/main/java` (all thirteen of them, across the nine modules that
are not examples) carries a
`package-info.java` with `@NullMarked` (`org.jspecify.annotations`): every field, parameter and
return type in the package is non-null unless explicitly annotated `@Nullable`. Nullness is
jspecify's job alone in this codebase — see the next paragraph for why `org.jetbrains.annotations`
stays out of it despite also defining `@Nullable`/`@NotNull`.

- **Why jspecify, not JetBrains, for nullness.** Both libraries define nullability annotations;
  using both for the same purpose would be redundant vocabulary for the same audience (a strict
  reviewer's first question). jspecify is the vendor-neutral, JSR-305-successor standard
  ([jspecify.dev](https://jspecify.dev)) that javac, IntelliJ/RustRover, the Checker Framework,
  NullAway and Error Prone all read natively, so it is the one every module declares.
  `org.jetbrains.annotations` still earns its own dependency (see `gradle/libs.versions.toml` and
  `terrace-config.java-conventions.gradle.kts`) for the inspection vocabulary jspecify doesn't
  cover: `@Contract` (used on `ContractAssembler`'s pure `deriveConstraint` helper — imported by
  fully-qualified name there, since this codebase's own `Contract` model type would otherwise
  collide with the annotation's simple name), and `@Blocking` (used on every method that performs
  real file I/O: both `ContractCodec.write(Contract, Path)`/`read(Path)` copies,
  `MetaSchemaValidator.load()`, and `FileIndirectionEnvironmentPostProcessor`'s file read).
- **Dependency shape.** jspecify's own guidance
  ([jspecify.dev/docs/using](https://jspecify.dev/docs/using/)) is `implementation` (or `api` under
  `java-library`), not `compileOnly`, since its annotations carry `RUNTIME` retention and the jar
  is tiny — every module takes it that way except `terrace-config-annotations`, which declares it
  `compileOnly` instead as a deliberate, narrow exception to jspecify's own advice: that module's
  entire reason to exist is costing a consumer nothing beyond the JDK (see its own
  `build.gradle.kts`), and every type in it is a `SOURCE`-retention `@interface` anyway, where
  `@Nullable` cannot apply to an annotation element in the first place.
  `org.jetbrains.annotations` is `compileOnly` everywhere without exception — confirmed against the
  library's own source, every annotation in it is `CLASS`-retention, so it is never a runtime
  requirement, not even reflectively.
- **Lombok interaction.** jspecify's `@Nullable` is `TYPE_USE`-only, written on the field itself
  (`@Nullable String foo;`, not above it), so a bare field-level annotation would stop at the field
  and never reach a Lombok-generated getter return type, builder-setter parameter, or
  all-args-constructor parameter — the actual public API those classes expose. `java/lombok.config`
  lists `org.jspecify.annotations.Nullable` under `lombok.copyableAnnotations` so Lombok copies it
  onto every generated signature it belongs on.
- **What actually ended up `@Nullable`.** Only fields/parameters/returns with a real "this can
  genuinely be absent" story: a model field the meta-schema itself marks optional (`Key.env`,
  `ExternalVar.owner`, `App.version`, ...), a private helper's `null`-sentinel return
  (`SchemaAssembler`'s constraint builders, `Origin.fromSources`, `TomlLayers.fragmentKeys`), an
  `Object.equals(Object)` override's parameter (`JsonSchemaDocument`, per the method's own
  contract), and `ProducerArguments.optional`'s absent option. Two narrow, documented exceptions: (1) the four fields
  `TerraceConfigProcessor` assigns in its overridden `init` rather than a constructor stay
  non-null by convention, the same "initializer method" exemption NullAway grants framework
  lifecycle methods — see that package's own `package-info.java`; (2) `lombok.NonNull` (a
  *runtime* constructor/builder guard, an orthogonal concern to jspecify's *static* contract) has
  been removed everywhere it previously stood in for a nullness *declaration* — a required field
  is now non-null by the enclosing package's `@NullMarked` default alone, with no runtime
  enforcement beyond what Jackson/the caller already provides.
- **A correctness fix surfaced along the way.** Formalising `ContainerShape.element()` as
  `@Nullable` (true of its type — the `NONE` sentinel constructs it with `null`) exposed a latent
  invariant `FieldResolver.resolveContainerField` relied on silently: `element()` is only ever
  non-null when `kind() != NONE`, which is exactly the one branch that method runs under. It now
  asserts that explicitly (`IllegalStateException` if it's somehow not, documented as unreachable)
  instead of leaving the guarantee implicit.

## Dual Jackson 2 / 3 support

Jackson 3.0 (GA'd 2025, driven by Spring Boot 4's own move to it) renamed its group ID and Java
packages from `com.fasterxml.jackson` to `tools.jackson` — except `jackson-annotations`, which
deliberately stayed on `com.fasterxml.jackson` 2.x so both majors can depend on it. Because of that
rename, a 2.x and a 3.x Jackson artifact are not two versions of "the same" dependency that
Gradle/Maven needs to reconcile — they're different coordinates entirely and coexist on one
classpath without conflict. That fact is what makes the actual design here possible: one model,
usable under either major, rather than two model copies.

- **The model in `-core` carries dual annotations.** `@Jacksonized` (Lombok's add-on for
  `@Builder`) normally emits a *single* `@JsonDeserialize(builder = ...)`/`@JsonPOJOBuilder` pair,
  and those two annotation types live in Jackson's `databind` package — `com.fasterxml.jackson`
  for 2.x, `tools.jackson` for 3.x — so by default a Lombok-built model only deserialises under
  whichever major it was generated for. Lombok 1.18.44 added
  `lombok.jacksonized.jacksonVersion`, which makes `@Jacksonized` emit *both* pairs at once; this
  repo's `java/lombok.config` sets it to both `2` and `3`. `terrace-config-core` itself declares
  both Jackson databind artifacts as `compileOnly` (never `implementation` — the model needs the
  annotation *types* resolvable at compile time, not either `ObjectMapper` at runtime), pinned in
  `gradle/libs.versions.toml`.
- **The actual `ObjectMapper` lives in two sibling codec modules**, `terrace-config-core-jackson2`
  and `terrace-config-core-jackson3` (see above), so that depending on `-core` alone pulls in
  neither Jackson major, and a consumer adds exactly one codec module — whichever major it already
  has — as an `implementation` dependency.
- **`terrace-config-spring-boot`** declares no Jackson dependency at runtime, directly or pinned —
  at runtime it only ever touches JSON through Spring's own `Binder`/`ObjectMapper`, so it
  automatically follows whichever Jackson major the consuming app's own Spring Boot BOM resolves
  (2.x under Boot 3, 3.x's `tools.jackson` coordinates under Boot 4) without needing a
  `-spring-boot4` sibling. `SpringContractMain`, which runs at build time, writes the contract with
  `-core-jackson2`: it compiles against it `compileOnly`, checks for it before first use, and
  finds it on the Gradle plugin's `terraceContractGenerator` classpath, never on the service's.

## `terrace-config-annotations` and `terrace-config-processor`

The Java answer to `#[derive(Describe)]` (see `rust/docs/SCHEMA.md`): a service annotates its own
configuration types, and `terrace-config-processor` (a JSR-269 annotation processor) generates a
sibling `<Type>Descriptor` class per annotated type, exposing `public static final TypeDescriptor
DESCRIPTOR` — one new record type in `de.timscho.config.core.descriptor`
(`TypeDescriptor`/`KeyDescriptor`/`ElementDescriptor`/`RangeConstraint`). A descriptor is
deliberately **dialect-agnostic** — no environment spelling, no alias derivation, no `text_form` —
a loader or Spring producer combines it with its own dialect to build the actual `Key` model in
`de.timscho.config.core.model`; that split is what keeps the processor usable by both.

- **`@TerraceConfig`** marks a class (or record) whose fields become keys, or an enum whose
  constants become the values one key accepts. **`@Nested`** recurses into a field's own
  `@TerraceConfig`-annotated type; **`@Values`**/`@Values(from = ...)`/`@Values({"a", "b"})` report
  a field's accepted spellings — bare, from a mirror enum, or a literal list, exactly the three
  forms `SCHEMA.md`'s `values`/`values_from`/`values(...)` draw; **`@Range`** bounds a numeric
  field; **`@Note`** and **`@Secret`** and **`@Skip`** match their Rust equivalents one for one.
  **`@Element`**/**`@ElementValues`** ask the same `@Nested`/`@Values` questions one level into a
  container's element (`Optional`/`List`/`Set`/`Map`, found by unwrapping generics — a map's *key*
  type is skipped, since a TOML table's keys are strings whatever the map is keyed by).
- **The compile error is the feature**, exactly as `SCHEMA.md` insists: a field whose type is
  none of the recognised leaves (`String`, the primitives and their boxes, `BigInteger`,
  `BigDecimal`) and carries none of the resolving annotations fails the build, naming the field,
  its type, and the attributes that would resolve it — not a key silently published with no shape.
  The same diagnostic covers a container whose element publishes nothing, a `@Range` on a
  non-numeric type or with no bound set, a `@Nested`/`@Values` pointed at a type that isn't
  annotated the right way, and more than one shape annotation on one field.
- **Jackson is read, not duplicated.** A struct's `closed` flag — for a `@Nested` field's type and
  an `@Element` container's element type alike — and an enum constant's spelling
  are read off `com.fasterxml.jackson.annotation.JsonIgnoreProperties(ignoreUnknown = false)` and
  `@JsonProperty` respectively — via `AnnotationMirror`, by qualified name, so the processor takes
  no compile dependency on either Jackson major (`jackson-annotations` is shared by both, see
  above, but even that import is avoided).
- **A field's own initializer decides `KeyDescriptor.hasDefault`.** `VariableElement` alone can
  answer "does this field have a constant value" only for a `static final` field
  (`getConstantValue()`); an ordinary instance field like `private String port = "8080";` needs the
  compiler's `Trees` API (`com.sun.source.util.Trees`/`com.sun.source.tree.VariableTree`) to see
  whether its declaration carries an initializer at all — a package `jdk.compiler` exports
  unconditionally (unlike `com.sun.tools.javac.*`), so no `--add-exports` is needed. `FieldResolver`
  reads it once per field and bakes it into the generated `KeyDescriptor` as a compile-time
  constant; see `SchemaAssembler` above for the one place it is read back out.
- **Generated descriptors are always top-level classes**, even for a `@TerraceConfig` type nested
  inside another class: `Filer.createSourceFile` treats every dot in a name as a package
  separator, so `Outer.Inner` cannot become `Outer.InnerDescriptor` by simple concatenation — it
  would ask for a class in a package literally named `Outer`. `DescriptorNaming` instead joins
  every enclosing simple name with `$` (`Outer$InnerDescriptor`), living directly in the type's own
  package, and both the processor and field resolution use it consistently so cross-references
  between two generated descriptors (nested/values pointing at another `@TerraceConfig` type)
  compile.
- **Tested with `compile-testing`** (`TerraceConfigProcessorTest`): one compilation per resolvable
  shape (plain leaves, `@Nested`, all three `@Values` forms, `@Range`, `@Element`,
  `@ElementValues`, `@Skip`) and one per refusal, asserting the diagnostic text — 17 cases in all.

## `terrace-config-loader`

The vanilla five-layer loader, `de.timscho.config.loader` — the Java equivalent of the Rust
crate's own `Terrace`/`Dialect`/`provider::*`/`layers::*`, ported class for class rather than
redesigned, since the layering *is* the product this loader exists to offer.

- **`Dialect`** carries the prefix, nesting separator (`__`), `_FILE` suffix, and the set of
  reserved keys a file may never supply — immutable, `with`-style methods, matching the Rust
  builder's own consuming style. `keyPath`/`envSpelling` round-trip a key between its two
  spellings; both the separator and reserved-key matching are case-insensitive, for the same
  Windows-environment and mixed-case-file reasons the Rust doc comments give.
- **The three layers** — `TomlLayers` (one file, or every `*.toml` in a directory, sorted and
  deep-merged), `SecretsDir` (a directory of key-named files, dotfiles and non-regular entries
  skipped, symlinks followed so a Kubernetes projected `Secret` volume works), `FileSuffixEnv`
  (`<PREFIX><KEY>_FILE=/path` indirection, scanned rather than looked up since the keys are
  open-ended) — each produce a nested `Map<String, Object>` rather than a `figment::Provider`,
  since there is no figment equivalent on the classpath; `LayerValues` holds the shared
  `insertNested`/`deepMerge`/`readValue` (UTF-8-strict, trailing `\r`/`\n` trimmed, never spaces)
  helpers every layer needs.
- **`ShadowPolicy`** (`REJECT`, the default, or `LAST_WINS`) and **`FileLayers`** — collecting the
  secrets-directory and indirection layers together so the shadow check can see both at once, and
  refusing (under `REJECT`) a key supplied by more than one of the environment, the secrets
  directory, or indirection — are ported one for one from `layers.rs`, error messages included.
- **`TerraceLoader`** is the builder: `configVar`/`secretsDirVar`/`defaultConfigPath`/
  `fileSuffix`/`nestingSeparator`/`reserve`/`shadowPolicy`, then `load(Class<T>)`. Layers merge
  lowest-precedence first — TOML, then prefixed environment variables (excluding reserved names
  and `_FILE` indirections, which are this loader's own mechanism, not configuration), then the
  file-backed layers on top — into one nested map, bound to the caller's type with a plain
  Jackson `ObjectMapper.convertValue`, an internal detail exactly as `-core`'s own Jackson
  dependency is. A second `load(Class<T>, Map<String, String>)` overload takes an explicit
  environment map instead of `System.getenv()` — the seam the test suite uses instead of actually
  mutating the process environment.
- **A real bug, found and fixed before it shipped**: `tomlj`'s `TomlTable.toMap()` does not
  recursively convert nested tables — a nested `[section]` stays a live `TomlTable` object rather
  than becoming a plain `Map`, which Jackson then reads through bean introspection instead of as a
  JSON object, surfacing as a spurious `"empty"` property (from `TomlTable`'s own `isEmpty()`)
  wherever a contract had non-flat TOML. Fixed with `TomlLayers`' own recursive
  `TomlTable`/`TomlArray` → `Map`/`List`/scalar converter, using the public `keySet()`/`get()`
  accessors rather than trusting `toMap()`.
- **Tested with real files in a JUnit `@TempDir`** rather than the corpus, since this loader has
  no `Contract`-shaped output to compare against a stored case yet: `DialectTest` (8 cases, ported
  from `dialect.rs`'s own unit tests) and `TerraceLoaderTest` (8 cases) — a TOML file alone, an
  environment variable overriding a TOML value, a secrets-directory file, `_FILE` indirection, a
  shadowed key rejected under `REJECT`, the same key resolved under `LAST_WINS`, a reserved key
  refused from a secrets file, and a missing TOML file being silently skipped rather than an
  error.
- **`schema()`/`schemaAt(descriptor, root)`** describe every key a `TypeDescriptor` (usually a
  generated `<Type>Descriptor.DESCRIPTOR`) can carry, spelled in this loader's own dialect,
  mirroring `Terrace::schema`/`schema_at` — see `SchemaAssembler` under `terrace-config-core`,
  below, for the algorithm; this method just supplies the loader's own dialect settings and the
  three loader-read variables (`config`, `secrets_dir`, each `reserve`d key) as `LoaderVar`s, the
  half no descriptor can see.
- **`explain()`/`explain(Map<String, String>)`** report which layer supplied every key this
  loader can see, without binding to any type — the Java `Terrace::explain`, ported field for
  field from `explain.rs`'s `Layer`/`Fragment`/`Origin`/`Explanation`. `Layer` is a sealed
  interface (`Toml`, `Env`, `SecretsFile`, `Indirection`) rather than the Rust `enum`, giving the
  same exhaustiveness at compile time; `Origin` names the effective layer plus every layer that
  lost, lowest precedence first; `Explanation.toString()` renders the same report the Rust crate's
  `Display` impl does (prefix, key count, contested count, one header line per layer, then every
  key indented under `keys:`), and, like the Rust type, **carries no configuration value** — only
  paths and variable names — so logging one is safe by construction. A shared package-private
  `TerraceLoader.Layers` record now holds each layer's own unmerged read, used by both `assemble`
  (which merges it for `load`) and `Explanation.of` (which reports on it instead), so the two can
  never disagree about what was actually read.
- **Tested with `ExplanationTest`** (7 cases): a TOML-only value with no shadowing, an environment
  override reported as shadowing the TOML value, a secrets-directory file attributed by its own
  path, `_FILE` indirection attributed to both the indirection variable and the file it named, a
  missing TOML file reported as `missing` rather than an error, an unreadable TOML file reported as
  `not valid TOML` with no parse detail leaked, and the full rendered report's header lines.
- **`loadWatched(Class)`/`loadWatched(Class, Map<String, String>)`** return a `Loaded<T>` (the
  bound value plus a `Sources`) instead of just the value — the Java `Terrace::load_watched`.
  `FileSuffixEnv.watchPaths()`/`FileLayers.watchPaths()` collect the *parent* directory of every
  file-backed input (the secrets directory, each `_FILE` indirection target), not the file
  itself, since a Kubernetes volume update replaces a file by renaming a whole new `..data`
  directory over the old one — a watch on the old file's inode never fires again.
  `Sources.differsFrom` compares the pre-binding merged map structurally, with `Double`/`Float`
  compared by raw bits rather than `==`, so a configuration holding `NaN` compares equal to
  itself instead of every reload looking like a change. `Sources.toString()` never prints the
  fingerprint — it is every configuration value, secrets included.
- **`ContractMain`** emits a service's contract at build time without starting it: given the root
  type's binary name, the prefix and `app.name`, it builds the schema with the same
  `TerraceLoader` settings the service boots with (`--reserve`, repeatable, `--nesting-separator`,
  `--file-suffix`, `--config-var`, `--secrets-dir-var`, `--default-config-path`), fills in the
  defaults from a default-constructed instance converted through Jackson, assembles the contract
  and writes it to `--out` or standard output. Build identity (`app.version`, `revision`,
  `created`, `source`) is deliberately not an option: it is `terrace-contract stamp`'s, so the
  emission is a pure function of the types and can be compared with a committed copy. Run by the
  Gradle plugin; by hand it is:

  ```bash
  java -cp <runtime classpath> de.timscho.config.loader.ContractMain \
      --type com.example.Config --prefix MYAPP_ --app-name my-service \
      --reserve MYAPP_PROFILE --out build/terrace/contract.unstamped.json
  ```

  `ContractMainTest` proves the emission byte-identical to assembling the same contract in code,
  and that every option reaches the document.

## `terrace-config-spring-boot`

The Spring Boot side, `de.timscho.config.spring` — deliberately not a second copy of
`terrace-config-loader`'s five layers. Spring's own `Binder`/`ConfigDataEnvironmentPostProcessor`
already give an application every layer the vanilla loader has to build by hand (files, profiles,
relaxed environment binding); this module only adds what Spring genuinely lacks.

- **`FileIndirectionEnvironmentPostProcessor`** is the one layer Spring doesn't already have:
  `<NAME>_FILE=/path` naming a file whose contents supply the property `<NAME>` would otherwise
  have named directly — the same convention `terrace-config-loader`'s `FileSuffixEnv` implements
  for the vanilla loader, and the one a mounted Kubernetes `Secret` or Docker secret is addressed
  by. It runs once, early (`EnvironmentPostProcessor`s execute before any bean — including the
  application's own — exists), scanning the environment's own `systemEnvironment` property source
  for `_FILE`-suffixed names, reading each file it finds (UTF-8-strict, trailing `\r`/`\n` trimmed,
  never spaces), and publishing the results in a new property source added at the *front* of the
  environment — so a mounted secret always outranks a plain environment variable or a
  configuration file, the same "file layers win" precedence `terrace-config-loader`'s own
  `FileLayers` documents. Registered via `META-INF/spring.factories`, since `EnvironmentPostProcessor`
  is one of the few extension points Spring Boot 3 still wires that way rather than through
  `AutoConfiguration.imports` — this class has to run before that mechanism even exists.
- **`SpringDialect`** names the one real divergence from `terrace-config-loader`'s own `Dialect`,
  rather than leaving it implicit: Spring's relaxed binding already treats a single `_` as the
  environment's nesting separator (`MYAPP_GITHUB_TOKEN` binds `myapp.github.token`), where the
  vanilla loader deliberately uses `__` so a field may itself be named with an underscore without
  colliding with nesting. This module cannot change Spring's own convention without breaking every
  property Spring already binds correctly, so the ambiguity is real and permanent for this
  producer — documented and exercised by `SpringDialectTest` rather than left as a surprise.
- **`SpringContractProducer.produce`** is the `Contract` producer: it wires `SpringDialect` into a
  `de.timscho.config.core.model.Dialect` (same prefix, `_` nesting, `_FILE` suffix), hands a
  `TypeDescriptor` and that dialect to `-core`'s `SchemaAssembler`, and passes the resulting
  `Schema` to `-core`'s `ContractAssembler` alongside the caller's own `App` — the same three-step
  composition `terrace-config-loader`'s `schemaAt` uses for its own `Schema`, one level further.
  `schema.loader` is always empty for this producer: Spring's `Binder` decides what the layers are
  from its own property sources, unlike the vanilla loader's `config`/`secrets_dir` variables, so
  there is nothing to publish there. Called at build time by `SpringContractMain`, below.
- **`SpringContractMain`** emits the contract at build time with no application context: given
  the properties class's binary name (nested types included), the prefix and `app.name`, plus
  `--nesting-separator`/`--file-suffix` for a dialect other than Spring's own, it reads the
  defaults off a default-constructed instance (what the binder leaves a field at when nothing
  supplies it), converted through Jackson, and writes the contract to `--out` or standard output.
  Nothing the service does at boot happens while its build emits the contract. It needs
  `terrace-config-core-jackson2` on the classpath it runs with (see "Dual Jackson 2 / 3 support"
  above) and says so when it is missing. There is deliberately no boot-time producer: a contract
  is a build artefact that describes the image, is conformed before the image exists, and ships
  inside it, and one assembled by a running application would be a second, unconformed copy.
- **Tested with a `MockEnvironment`** (`spring-test`) standing in for the real system environment
  property source, not a booted `SpringApplication`: `SpringDialectTest` (7 cases) and
  `FileIndirectionEnvironmentPostProcessorTest` (7 cases — a file supplying its target, trailing
  newline trimmed but not other whitespace, a plain variable left alone, no indirection meaning no
  property source added, a missing file and an invalid-UTF-8 file each refused naming the
  variable, and a file-backed value outranking an ordinary property already present).
  `SpringContractProducerTest` (2 cases) proves the whole path against a real generated descriptor
  (`ServiceConfig`/`ServiceConfigDescriptor`): a validated `Contract` with the right dialect,
  secret and env spelling, and the empty-prefix refusal still firing through `ContractValidator`.
  `SpringContractMainTest` (3 cases) proves `SpringContractMain`'s emission byte-identical to
  `SpringContractProducer.produce` with the defaults a default-constructed instance converts to,
  and that the dialect options and a missing `--app-name` behave as documented.

## `terrace-config-gradle-plugin`

The `de.timscho.terrace-config` Gradle plugin, `de.timscho.config.gradle`: a service's contract,
emitted from its own types at build time, stamped with the build's identity, conformed by the real
`terrace-contract`, and kept equal to the copy committed beside the service. It applies `java`, and
it is all a service's build needs:

```kotlin
plugins {
    id("de.timscho.terrace-config")
}

terraceContract {
    type = "com.example.Config"   // the root @TerraceConfig type, by binary name
    prefix = "MYAPP_"
    reserve.add("MYAPP_PROFILE")  // terrace-java only
}

// Spring Boot only: the codec SpringContractMain writes with, on the generator's classpath alone.
dependencies {
    terraceContractGenerator("com.github.TimSchoenle.terrace-config:terrace-config-core-jackson2:<tag>")
}
```

| Task | What it does |
|---|---|
| `terraceContractEmit` | Runs `ContractMain` or `SpringContractMain` on the main source set's runtime classpath (plus `terraceContractGenerator`), on the project's Java toolchain, and writes `build/terrace/contract.unstamped.json`. Cacheable: its inputs are the classes, the entry point, its options and the JVM, and no stamped value, so a new commit or a new timestamp never invalidates it. |
| `terraceContract` | `terrace-contract stamp` over that document, then `terrace-contract conform --tier <n>` over the result, written to `build/terrace/contract.json`. A document that does not conform is deleted. |
| `terraceContractCheck` | Fails when the committed `contract.json` is not byte-for-byte the unstamped emission, naming the first differing line. `check` depends on it. |
| `terraceContractUpdate` | Rewrites the committed `contract.json` with the unstamped emission. |

The `terraceContract { }` extension:

| Property | Default |
|---|---|
| `type`, `prefix` | none; both are required |
| `appName` | the project's name |
| `reserve` | empty; each is passed as `--reserve` (`terrace-java` only) |
| `producerArguments` | empty; passed to the entry point as they are, for its other options (`--nesting-separator`, `--file-suffix`, `--config-var`, ...) |
| `loader` | `terrace-java` when `terrace-config-loader` is on the runtime classpath, `spring-boot` when `terrace-config-spring-boot` is; required when both or neither is |
| `tier` | the producer's claim: 2 for `terrace-java`, 1 for `spring-boot` |
| `appVersion` | `-PterraceContract.appVersion`, else the project's version unless it is `unspecified` |
| `revision` | `-PterraceContract.revision`, else `TERRACE_CONTRACT_REVISION` |
| `created` | `-PterraceContract.created`, else `TERRACE_CONTRACT_CREATED`; unset by default, so two builds of one commit stamp the same document |
| `source` | unset |
| `executable` | `TERRACE_CONTRACT_BIN` (a relative path is resolved against the root project), else `PATH`, and nothing else: no directory nobody named is searched, and nothing is downloaded, since the binary that holds a build's contract to the specification is one the build's owner chose |
| `output` | `build/terrace/contract.json` |
| `committedContract` | `contract.json` in the project directory |

The committed copy is compared with the unstamped emission, not the stamped one, so it carries no
version, commit or timestamp and changes only when the configuration surface does. The plugin is
compiled for Java 17, the floor Gradle 9 runs on. Each example's `Dockerfile` shows the rest of the
path: a build stage that copies `terrace-contract` from `ghcr.io/timschoenle/terrace-contract`,
runs `terraceContract`, and a runtime stage that copies the stamped contract to
`/config/contract.json` and carries the label block `terrace-contract render --format dockerfile`
prints for it.

- **Tested with Gradle TestKit** (`TerraceContractPluginFunctionalTest`, 11 cases), in real builds
  whose stub entry points copy a corpus document to `--out` and record their options: stamping
  and conforming with the configuration cache on, the emission staying cached across stamp
  changes, the check and update round trip, the producer and tier inferred from the classpath,
  and the messages for a refusal, a document below its tier and a missing binary.
  `ExecutableLocatorTest` and `TerraceContractCheckTest` cover the lookup order and the difference
  message.

## Contributing

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org).

`java/README.md` is generated. Edit `.github/templates/java-README.md.hbs` instead. CI renders it on every
pull request and commits the result back to the branch, and a push to `main` whose `java/README.md`
does not match its template fails.

The gates a pull request has to pass are in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml);
all of them run locally:

```bash
cd java
TERRACE_CONTRACT_BIN=/path/to/terrace-contract ./gradlew check terraceContract
```

`check` and `terraceContract` need the `terrace-contract` binary; see "Building and testing" above.

## Security

[SECURITY.md](../SECURITY.md) has the reporting instructions. Do not open a public issue for a
vulnerability.

## Licence

`MIT`. See [LICENSE](../LICENSE) for the terms.
