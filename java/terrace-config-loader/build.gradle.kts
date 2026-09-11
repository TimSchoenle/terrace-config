// The vanilla five-layer loader (PR 6): type defaults, a TOML file/directory, prefixed environment
// variables, a directory of key-named files, and _FILE indirection. `producer.loader` is
// `terrace-java`. Depends on -core for the model and -processor (compile-time only) for
// descriptors; knows nothing about Spring.
description = "terrace-config-java: the vanilla five-layer loader, target tier 2 — " +
    "layers, dialect and Jackson binding implemented; schema/explain wiring against the " +
    "processor's descriptors is open. See java/README.md."

plugins {
    id("terrace-config.lombok-conventions")
}

dependencies {
    implementation(project(":terrace-config-core"))
    implementation(project(":terrace-config-annotations"))
    implementation(libs.tomlj)
    // jspecify's own guidance, not `compileOnly` — see gradle/libs.versions.toml and
    // terrace-config-annotations/build.gradle.kts for the one deliberate exception to it.
    implementation(libs.jspecify)
    // The binder from a merged layer map to the caller's type. An internal detail, exactly as
    // `-core`'s own Jackson 2 dependency is -- not exposed on `TerraceLoader`'s public API.
    implementation(libs.jackson.databind)
    compileOnly(project(":terrace-config-processor"))
    annotationProcessor(project(":terrace-config-processor"))
    testCompileOnly(project(":terrace-config-processor"))
    testAnnotationProcessor(project(":terrace-config-processor"))
}
