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
| `terrace-config-core` | The envelope model, the eight refusals, the renderings. Knows about documents, not binders. | not started |
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

`terrace-config-spec-tck` is the only module with anything to test today: it reads
`../spec/v1/contract.schema.json` and `../spec/v1/conformance/*/contract.json` directly, so it
needs no producer and no other module to exercise for real. The other five modules are empty
placeholders — they compile, but there is nothing in them yet.

## Conventions

No streams (loops), Lombok for the model classes once they exist, minimal dependencies, and
deterministic-by-construction rendering — ordered collections and explicit property order rather
than relying on incidental iteration order. Comments explain why, not what. `terrace-config-spec-tck`
already follows all of them.

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

## Jackson: handling the 2.x / 3.x split

Jackson 3.0 (GA'd October 2025, driven by Spring Boot 4's own move to it) renamed its group ID and
Java packages from `com.fasterxml.jackson` to `tools.jackson` — except `jackson-annotations`,
which deliberately stayed on `com.fasterxml.jackson` 2.x so both majors can depend on it. Because
of that rename, a 2.x and a 3.x Jackson artifact are not two versions of "the same" dependency
that Gradle/Maven needs to reconcile — they're different coordinates entirely and coexist on a
classpath without conflict. That fact shapes the whole plan here, so no dependency-alignment
scheme is needed:

- **`terrace-config-core`** depends on `jackson-databind` 2.x directly, pinned in the version
  catalog, purely as an internal implementation detail of its own JSON rendering. No Jackson type
  from it is ever exposed on a public API, so a consumer's own Jackson version (2.x or 3.x,
  wherever it comes from) never has to match this module's.
- **`terrace-config-spring-boot`** declares no Jackson dependency at all, directly or pinned — it
  only ever touches JSON through Spring's own `Binder`/`ObjectMapper`, so it automatically follows
  whichever Jackson major version the consuming app's own Spring Boot BOM resolves (2.x under Boot
  3, 3.x's `tools.jackson` coordinates under Boot 4). This is the one module where the split can
  actually surface, since it's the one module bound to Spring's own Jackson instance rather than
  bringing its own.
- If Spring Boot 4 / Jackson 3 support is needed while Boot 3 / Jackson 2 support still has to
  ship, the plan is a sibling module — `terrace-config-spring-boot4` — with its own Spring Boot 4
  BOM pin, not a branch inside `terrace-config-spring-boot` or a runtime version check. `-core` and
  `-loader` need no equivalent split at all, since neither ever imports Spring's Jackson instance.
  Not built yet, since nothing in this repository needs it before `terrace-config-spring-boot`
  itself (PR 7) exists to fork from.
