// The document model and everything true of a contract regardless of loader. Knows about
// documents, not about binders: no Spring, and reflection over a user type only in
// `ConfigurationType`, for a build-time producer that is handed the type by name. Depended on by
// -loader, -spring-boot and -spec-tck; depends on nothing that knows about either.
description = "The envelope model, the build-time refusals, and the JSON Schema half a contract " +
    "embeds. Every other rendering is `terrace-contract render`'s. See java/README.md."

// Applying the plugin (rather than hand-rolled `compileOnly`/`annotationProcessor` lines) wires
// main *and* test sources, and points `javadoc` at delomboked sources automatically; see
// java/README.md's Lombok section and the root java/lombok.config.
plugins {
    id("terrace-config.lombok-conventions")
    id("terrace-config.publish-conventions")
}

// This module holds only the document model, never an `ObjectMapper` of either Jackson major;
// see `-core-jackson2`/`-core-jackson3` for the actual codecs. `@Jacksonized` (configured in
// java/lombok.config to target both Jackson majors) still needs both databind artifacts'
// `@JsonDeserialize`/`@JsonPOJOBuilder` annotation types resolvable at compile time, so both are
// declared `compileOnly` here, never `implementation`, since neither is a runtime requirement of
// the model by itself. See java/README.md's "Dual Jackson 2 / 3 support" section.
dependencies {
    compileOnly(libs.jackson.databind)
    compileOnly(libs.jackson.databind.v3)
    // jspecify's own guidance, not `compileOnly`; see gradle/libs.versions.toml and
    // terrace-config-annotations/build.gradle.kts for the one deliberate exception to it.
    implementation(libs.jspecify)
}

// Test sources reflectively load the Lombok-generated `@JsonDeserialize`/`@JsonInclude` etc.
// annotation classes too (e.g. to compile against Lombok's generated builders), so the test
// compile classpath needs the same `compileOnly` Jackson types main has, since the `java` plugin
// does not extend `testCompileOnly` from `compileOnly` on its own.
configurations.testCompileOnly.get().extendsFrom(configurations.compileOnly.get())
