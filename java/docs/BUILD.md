# Building and testing

How the Java build is put together: the wrapper and the files that pin versions, the convention
plugins in `buildSrc`, what each test suite proves, and the checks a pull request has to pass.

## Contents

- [Building](#building)
- [What the tests prove](#what-the-tests-prove)
- [The committed contracts](#the-committed-contracts)
- [The `terrace-contract` binary](#the-terrace-contract-binary)
- [Convention plugins](#convention-plugins)
- [Conventions](#conventions)
- [Formatting and Checkstyle](#formatting-and-checkstyle)
- [Lombok](#lombok)

## Building

```bash
cd java
./gradlew build
```

The wrapper (`gradlew`, `gradlew.bat` and `gradle/wrapper/`) is committed, so no local Gradle
install is needed: it downloads the version `gradle/wrapper/gradle-wrapper.properties` pins on
first run. Versions live in three files, each the only place its facts are stated:

| File | What it holds |
|---|---|
| `gradle.properties` | `group`, `version` and `javaVersion`, shared by every module. release-please rewrites `version`, and the README's JDK badge reads `javaVersion`. |
| `gradle/libs.versions.toml` | Every dependency and plugin version, for the modules and for `buildSrc` alike. |
| `gradle/wrapper/gradle-wrapper.properties` | The Gradle version. |

The toolchain is resolved by `org.gradle.toolchains.foojay-resolver-convention` (declared in
`settings.gradle.kts`), which downloads a matching JDK when the machine has none, so the JVM that
runs Gradle does not have to be the one the modules compile for.

## What the tests prove

Each suite tests what its module owns, against the most direct evidence available.

- **`terrace-config-core`, `-core-jackson2` and `-core-jackson3`** read
  `../spec/v1/conformance/*/contract.json` directly, so none needs a producer. Each codec module's
  `ContractCorpusRoundTripTest` deserialises every stored case into the shared model and
  serialises it again, asserting the result is byte-identical to the stored file. Running it once
  per Jackson major against the same model classes is the strongest check the model has.
  `ContractValidatorTest` has one test per refusal, each on a minimal `Contract`, so the refusal
  under test is isolated from the other eleven.
- **`terrace-config-processor`**'s `TerraceConfigProcessorTest` runs the processor through
  `compile-testing` against small in-memory sources: one compilation per resolvable shape and one
  per refusal, asserting the diagnostic text.
- **`terrace-config-loader`**'s suites exercise real files in a JUnit `@TempDir`: a TOML fragment,
  a secrets directory, `_FILE` indirection, shadowing under both policies, a reserved key, and the
  explanation report. `load(Class, Map)` takes the environment as a map, so no test mutates the
  process environment. `ContractMainTest` proves the build-time emission byte-identical to
  assembling the same contract in code.
- **`terrace-config-spring-boot`**'s suites use a `MockEnvironment` from `spring-test`, because
  an `EnvironmentPostProcessor` runs before there is a context to boot. `SpringContractProducerTest`
  runs the whole path against a real generated descriptor, and `SpringContractMainTest` proves the
  emission byte-identical to `SpringContractProducer.produce`.
- **`terrace-config-spec-tck`** validates every stored case against `contract.schema.json` and its
  schema-only sub-schema, and `JavaConformanceTest` renders real `@TerraceConfig` fixtures through
  the loader and the Jackson 2 codec. They are checked at tier 1 (the meta-schema), tier 2 (dialect
  spellings against the shared spec corpus) and tier 3 (byte for byte against this module's own
  `conformance-java/` corpus). Regenerate that corpus with
  `./gradlew :terrace-config-spec-tck:blessJavaConformance`. `JacksonParityTest` proves the two
  codecs render the same bytes.
- **`terrace-config-gradle-plugin`**'s `TerraceContractPluginFunctionalTest` runs real builds
  through Gradle TestKit, with stub entry points that copy a corpus document to `--out` and record
  their options. It covers stamping and conforming with the configuration cache on, the emission
  staying cached across stamp changes, the check and update round trip, the producer and tier
  inferred from the classpath, and the messages for a refusal, a document below its tier and a
  missing binary.
- **`terrace-config-example-service`**'s `OrdersServiceConfigTest` loads the exact `Config` class
  the service boots with, every way `TerraceLoader` supports.
- **`terrace-config-example-spring-service`** splits in two. `OrdersPropertiesBindingTest` binds
  `OrdersProperties` through `ApplicationContextRunner`, a lightweight context.
  `OrdersServiceFileIndirectionIntegrationTest` boots a real `SpringApplication`, swapping in a
  fake `SystemEnvironmentPropertySource` before `run()`, because `_FILE` indirection is an
  `EnvironmentPostProcessor` and a context runner never invokes one.

## The committed contracts

Both example modules apply the `de.timscho.terrace-config` plugin. Its `terraceContractCheck`,
part of `check`, emits a fresh contract from the module's annotated type and compares it byte for
byte with the `contract.json` committed in the module's directory. That is the same guarantee
`cargo test` gives the Rust crate's `examples/service/`. Regenerate both with
`./gradlew terraceContractUpdate`, or one with `./gradlew :<example>:terraceContractUpdate`.

A key whose Java name changes case in a way the environment fold cannot reverse, such as
`bindAddr`, renders with `"env": null` and `"unreachable": "unnameable"` rather than a spelling
nothing would respond to. The loader example's `Config` avoids it with `@JsonProperty`, which the
processor reads. The Spring example's `OrdersProperties` keeps the Java names, so `bindAddr`,
`database.maxConnections` and `logLevel` are unreachable in its contract even though Spring's
relaxed binding tolerates them.

## The `terrace-contract` binary

`TerraceContractConformTest` in `-spec-tck`, the plugin's functional tests and each example's
`terraceContract` task run the real `terrace-contract` binary.

- **The tests** look for it at `TERRACE_CONTRACT_BIN`, then `../cli/target/release/terrace-contract`,
  then `../cli/target/debug/terrace-contract`, then on `PATH`, and hand it to the builds they run
  through `TERRACE_CONTRACT_BIN`.
- **The examples' `terraceContract`** uses the plugin's own lookup, which is
  `TERRACE_CONTRACT_BIN` or `PATH` and nothing else.

Without a binary, the tests fail when `CI=true` and are skipped through a JUnit assumption
otherwise. CI cannot go green by skipping them, and a contributor without a Rust toolchain can
still run the rest. CI builds the binary from the same commit:

```bash
(cd ../cli && cargo build --release)
TERRACE_CONTRACT_BIN="$PWD/../cli/target/release/terrace-contract" ./gradlew check terraceContract
```

## Convention plugins

`java/buildSrc/` is a standalone Gradle build that compiles three convention plugins from
`buildSrc/src/main/kotlin/terrace-config.*-conventions.gradle.kts`. Every module applies them by
id instead of receiving configuration from a root `subprojects {}` block, so what a module gets is
stated in its own build file.

| Plugin | What it adds | Applied by |
|---|---|---|
| `terrace-config.java-conventions` | The toolchain, UTF-8 encoding, the documentation gate, JUnit 5 and AssertJ, the JetBrains annotations, Checkstyle and Spotless. | Every module, directly or through the Lombok plugin |
| `terrace-config.lombok-conventions` | The above, plus `io.freefair.lombok` with the catalog's Lombok version. | `-core`, `-loader`, `-spring-boot` and both examples |
| `terrace-config.publish-conventions` | `com.vanniktech.maven.publish` with the licence, developer and SCM metadata, plus sources and Javadoc jars. A module that applies `java-gradle-plugin` first is published as a Gradle plugin with its marker. | The eight published modules; see [PUBLISHING.md](PUBLISHING.md) |

`buildSrc/settings.gradle.kts` points `buildSrc`'s own catalog at `../gradle/libs.versions.toml`,
so there is one file to bump a version in. A precompiled script plugin cannot use the type-safe
`libs.xxx` accessors, because Gradle cannot know the catalog shape of the project it will be
applied to. The convention plugins use the string-keyed `VersionCatalogsExtension` lookup
(`libs.findLibrary("...")`, `libs.findVersion("...")`) instead, and module build files keep the
type-safe accessors.

`buildSrc` also compiles `terrace-config-gradle-plugin`'s sources a second time and registers the
same `de.timscho.terrace-config` id. A build cannot apply a plugin one of its own subprojects
publishes, and `buildSrc`'s classes are on every module's build script classpath, so this is how
the two examples apply the plugin by id. The copy the examples run and the copy JitPack publishes
come from one set of sources.

## Conventions

- No streams; loops instead.
- Lombok for the model classes.
- Minimal dependencies.
- `@NullMarked` packages, with jspecify `@Nullable` for genuine absence. See
  [NULL_SAFETY.md](NULL_SAFETY.md).
- Rendering is deterministic by construction: ordered collections and explicit property order,
  never incidental iteration order.
- Concrete classes are `final`. The one exception is a `@SpringBootApplication` class, which Spring
  subclasses at startup.
- Comments explain why, not what.

The documentation gate runs in both halves of the toolchain. `compileJava` runs doclint with
`-Xdoclint:all/protected -Werror`, and the `javadoc` task runs it again at the protected level
with warnings as errors, because the two catch different problems. It covers main sources only:
test sources are never published. A Lombok class carries `@SuppressWarnings("doclint:missing")`,
since its generated members have no comments of their own and its fields document them.

## Formatting and Checkstyle

Every module gets [Spotless](https://plugins.gradle.org/plugin/com.diffplug.spotless) and
Checkstyle from `terrace-config.java-conventions`, so neither depends on a contributor's IDE
settings.

- **`palantirJavaFormat`**, at the version in the catalog's `palantir-java-format` entry, which
  Spotless resolves at format time without putting it on any module's classpath. It keeps 4-space
  indentation and a 120-column width.
- **`formatAnnotations()`** keeps a `TYPE_USE` annotation such as jspecify's `@Nullable` inline on
  the type it annotates, where `palantirJavaFormat()` alone moves it to its own line like a
  declaration annotation.
- **`importOrder("")`** puts every import in one alphabetical group with no blank lines, which is
  what the Checkstyle ruleset's `ImportOrder` accepts. `removeUnusedImports()`,
  `trimTrailingWhitespace()` and `endWithNewline()` run alongside it.
- **Checkstyle** runs the `library` tier ruleset from `TimSchoenle/actions`, resolved from JitPack
  at the version the catalog pins. `checkstyleTest` uses `config/checkstyle/checkstyle-test.xml`.

CI runs `./gradlew spotlessCheck checkstyleMain checkstyleTest` in the same step as `test`. Run
`./gradlew spotlessApply` to fix a formatting violation.

## Lombok

`terrace-config.lombok-conventions` applies the
[`io.freefair.lombok`](https://plugins.gradle.org/plugin/io.freefair.lombok) plugin, so a module
opts in with one `id("terrace-config.lombok-conventions")` line. The plugin wires
`compileOnly` and `annotationProcessor` for main and test sources, and points `javadoc` at
delomboked sources so the generated documentation shows real method signatures.

The other modules apply `terrace-config.java-conventions` alone. Three of them say why in their
`build.gradle.kts`:

- `terrace-config-annotations`: annotation types cannot carry Lombok annotations, and the module
  promises a consumer nothing beyond the JDK.
- `terrace-config-processor`: its code is plain `javax.annotation.processing` and
  `javax.lang.model` logic, and the descriptors it emits reference `-core`'s records.
- `terrace-config-spec-tck`: its classes are not bean-shaped.

Shared behaviour lives in `java/lombok.config`, which Lombok finds by walking up from each source
file. `config.stopBubbling = true` stops that walk at `java/`. It sets `@Generated` on generated
members so coverage tools skip them, `callSuper = call` on `@EqualsAndHashCode` and `@ToString`,
both Jackson majors for `@Jacksonized` (see [JACKSON.md](JACKSON.md)), and jspecify's `@Nullable`
as a copyable annotation (see [NULL_SAFETY.md](NULL_SAFETY.md)).

It also sets `lombok.fieldDefaults.defaultPrivate = true`, which applies to every field with no
access modifier in a Lombok-processed module, not only to fields of Lombok-annotated classes. A
package-private field that another class in the package reads needs an explicit modifier. That is
why `Node`, which `JsonSchemaRenderer` reads, declares its fields `public`; `Node` itself is
package-private.
