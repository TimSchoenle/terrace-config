// The JSR-269 annotation processor generating descriptors from @TerraceConfig-annotated types
// (PR 5). Depends on -annotations to read the vocabulary and -core to emit its model, but nothing
// a compiled service needs at runtime — this module runs only during annotation processing.
description = "The @TerraceConfig annotation processor, mirroring #[derive(Describe)]. " +
    "See java/README.md."

// No Lombok here: the processor's own code is plain javax.annotation.processing/javax.lang.model
// logic, and the descriptors it emits reference -core's plain Java records
// (TypeDescriptor/KeyDescriptor/...), never Lombok-annotated classes of its own.
plugins {
    id("terrace-config.java-conventions")
}

dependencies {
    implementation(project(":terrace-config-annotations"))
    implementation(project(":terrace-config-core"))
    testImplementation(libs.compile.testing)
    // jspecify's own guidance, not `compileOnly` — see gradle/libs.versions.toml and
    // terrace-config-annotations/build.gradle.kts for the one deliberate exception to it.
    implementation(libs.jspecify)
}
