// The vanilla five-layer loader (PR 6): type defaults, a TOML file/directory, prefixed environment
// variables, a directory of key-named files, and _FILE indirection. `producer.loader` is
// `terrace-java`. Depends on -core for the model and -processor (compile-time only) for
// descriptors; knows nothing about Spring.
description = "terrace-config-java: the vanilla five-layer loader, target tier 2 — " +
    "not started yet (PR 6). See java/README.md."

plugins {
    id("terrace-config.lombok-conventions")
}

dependencies {
    implementation(project(":terrace-config-core"))
    implementation(project(":terrace-config-annotations"))
    compileOnly(project(":terrace-config-processor"))
    annotationProcessor(project(":terrace-config-processor"))
}
