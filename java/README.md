# terrace-config (Java)

The Java implementations of the configuration contract at [`spec/`](../spec/README.md). No tier is
claimed for anything yet — this module exists so that the harness checking a future producer can
be built and tested first.

See [`docs/migration-progress.md`](../docs/migration-progress.md) for what is actually done and
what remains open.

## Modules

| Module | What it is | Status |
|---|---|---|
| `terrace-config-annotations` | `@TerraceConfig` and the rest of the vocabulary a service's own types carry. Depends on nothing but the JDK. | not started |
| `terrace-config-core` | The envelope model and the eight build-time refusals. Knows about documents, not binders, and about neither Jackson major — see `-core-jackson2`/`-core-jackson3` for the codecs. | **implemented** — validation and a Jackson-version-agnostic model; the nine renderings (json, markdown, toml, json-schema, contract, labels, dockerfile, ...) live in the codec modules below |
| `terrace-config-core-jackson2` | The byte-stable `json` codec for the `-core` model, built on Jackson 2.x. | **implemented** |
| `terrace-config-core-jackson3` | The same codec, built on Jackson 3.x instead. | **implemented** |
| `terrace-config-processor` | The JSR-269 annotation processor generating descriptors from annotated types. | not started |
| `terrace-config-loader` | The vanilla five-layer loader. `producer.name` will be `terrace-config-java`, `producer.loader` `terrace-java`, target **tier 2**. | not started |
| `terrace-config-spring-boot` | The Spring Boot starter and producer over Spring's `Binder`. `producer.loader` will be `spring-boot`, target **tier 1**, divergence documented once it exists. | not started |
| `terrace-config-spec-tck` | The meta-schema validator and tier comparator, checked against the stored corpus in `spec/v1/conformance/`. | **implemented** — validates every stored case against `contract.schema.json` and its schema-only sub-schema; compares two documents at tier 1/2/3 |

## Building and testing

```bash
cd java
./gradlew test
```

The wrapper (`gradlew`/`gradlew.bat`) is committed, so no local Gradle install is needed — it
pulls the pinned version (9.1.0, the first release supporting Java 25) on first run. Module
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
The remaining four modules (`terrace-config-annotations`, `-processor`, `-loader`,
`-spring-boot`) are still empty placeholders — they compile, but there is nothing in them yet.

## Conventions

No streams (loops), Lombok for the model classes, minimal dependencies, and
deterministic-by-construction rendering — ordered collections and explicit property order rather
than relying on incidental iteration order. Comments explain why, not what.
`terrace-config-spec-tck` and `terrace-config-core` already follow all of them.

## `terrace-config-core`

The document model — `Contract` and everything it's built from (`de.timscho.config.core.model`)
— and the eight build-time refusals (`de.timscho.config.core.refusal`), all mirroring
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
  refuse" — all eight ways a contract could quietly stop being one — and throws the first
  violation as its own `ContractRefusalException` subclass. It lives here, once, rather than in
  each of `-loader` and `-spring-boot`, because both build the same `Contract` shape and must
  refuse the same eight things.
- **`ContractValidatorTest`** has one test per refusal (plus one proving a well-formed contract
  passes all eight), each built with a minimal `Contract` rather than a full corpus case, to keep
  the refusal under test isolated from the other seven. The byte-stable round-trip test lives in
  each codec module instead (see below), since it needs an actual `ObjectMapper` to run.

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
by hand) that compiles two *convention plugins* under
`buildSrc/src/main/kotlin/terrace-config.*-conventions.gradle.kts` and makes them available, by
ID, to every module below it:

- **`terrace-config.java-conventions`** — the Java 21 toolchain, UTF-8 source/Javadoc encoding,
  and the JUnit 5 + AssertJ test dependencies every module gets. Applied by all six modules,
  either directly (`terrace-config-annotations`, `terrace-config-spec-tck`) or transitively
  through the plugin below.
- **`terrace-config.lombok-conventions`** — the above, plus `io.freefair.lombok` pre-configured
  with the version from `gradle/libs.versions.toml`. Applied by `terrace-config-core`,
  `terrace-config-processor`, `terrace-config-loader` and `terrace-config-spring-boot`.

This replaces what used to be a root `java/build.gradle.kts` `subprojects { ... }` block: the same
settings, but expressed as plugins a module opts into by ID rather than configuration silently
applied to every project from outside. The root `java/build.gradle.kts` itself is now just the
shared `group`/`version` (`allprojects`), since there's nothing else left to centralize there.
`buildSrc/settings.gradle.kts` points buildSrc's own version catalog at the same
`gradle/libs.versions.toml` file the modules use, so there's still exactly one file to bump a
version in — but note that Gradle's type-safe `libs.xxx` accessors don't work *inside* a
precompiled script plugin (a documented limitation: the plugin might end up applied to a project
with a differently-shaped catalog), so the two convention plugins above use the lower-level,
string-keyed `VersionCatalogsExtension` lookup instead (`libs.findLibrary("...")`,
`libs.findVersion("...")`); ordinary module `build.gradle.kts` files keep using the type-safe
accessors as before.

## Lombok

The [`io.freefair.lombok`](https://plugins.gradle.org/plugin/io.freefair.lombok) Gradle plugin,
version pinned once in `buildSrc/build.gradle.kts`, is wired up by the
`terrace-config.lombok-conventions` plugin (see "buildSrc" above) rather than applied by hand in
each module — so a module opts in with a single `id("terrace-config.lombok-conventions")` line,
no repeated `lombok { version.set(...) }` block. It's applied (transitively) to every module that
has, or will have, bean-shaped classes: `terrace-config-core`, `terrace-config-processor`,
`terrace-config-loader` and `terrace-config-spring-boot`. Two modules deliberately opt out, each
for its own reason documented in its `build.gradle.kts`: `terrace-config-annotations` (annotation
types can't carry Lombok annotations, and the module's whole point is "nothing but the JDK"), and
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
- **`terrace-config-spring-boot`** will declare no Jackson dependency at all, directly or pinned —
  it only ever touches JSON through Spring's own `Binder`/`ObjectMapper`, so it automatically
  follows whichever Jackson major the consuming app's own Spring Boot BOM resolves (2.x under Boot
  3, 3.x's `tools.jackson` coordinates under Boot 4) without needing a `-spring-boot4` sibling —
  unlike the codec, there is no `ObjectMapper` of its own to fork.
