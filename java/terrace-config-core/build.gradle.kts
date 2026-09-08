// The document model and everything true of a contract regardless of loader (PR 4). Knows about
// documents, not about binders — no Spring, no reflection over user types. Depended on by
// -loader, -spring-boot and -spec-tck; depends on nothing that knows about either.
description = "The envelope model, the eight build-time refusals, and the `json-schema` " +
    "rendering (PR 4). The remaining renderings beyond `json` (markdown, toml, contract, " +
    "labels, dockerfile, ...) are not yet implemented. See java/README.md."

// Applying the plugin (rather than hand-rolled `compileOnly`/`annotationProcessor` lines) wires
// main *and* test sources, and points `javadoc` at delomboked sources automatically — see
// java/README.md's Lombok section and the root java/lombok.config.
plugins {
    id("terrace-config.lombok-conventions")
}

// This module holds only the document model, never an `ObjectMapper` of either Jackson major —
// see `-core-jackson2`/`-core-jackson3` for the actual codecs. `@Jacksonized` (configured in
// java/lombok.config to target both Jackson majors) still needs both databind artifacts'
// `@JsonDeserialize`/`@JsonPOJOBuilder` annotation types resolvable at compile time, so both are
// declared `compileOnly` here — never `implementation`, since neither is a runtime requirement of
// the model by itself. See docs/migration-progress.md, "Dual Jackson 2 / 3 support".
dependencies {
    compileOnly(libs.jackson.databind)
    compileOnly(libs.jackson.databind.v3)
}

// Test sources reflectively load the Lombok-generated `@JsonDeserialize`/`@JsonInclude` etc.
// annotation classes too (e.g. to compile against Lombok's generated builders), so the test
// compile classpath needs the same `compileOnly` Jackson types main has — the `java` plugin
// does not extend `testCompileOnly` from `compileOnly` on its own.
configurations.testCompileOnly.get().extendsFrom(configurations.compileOnly.get())

dependencies {
    // Used only by TomlExampleRendererTest, to prove the generated example actually parses --
    // never a main-source dependency, since -core's TOML rendering writes text, it never reads it.
    testImplementation(libs.tomlj)
}
