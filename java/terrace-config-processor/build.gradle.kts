// The JSR-269 annotation processor generating descriptors from @TerraceConfig-annotated types
// (PR 5). Depends on -annotations to read the vocabulary and -core to emit its model, but nothing
// a compiled service needs at runtime — this module runs only during annotation processing.
description = "The @TerraceConfig annotation processor, mirroring #[derive(Describe)] — " +
    "not started yet (PR 5). See java/README.md."

// Builds the descriptors it emits with the same Lombok the modules it feeds (-core, -loader) use
// for their own model classes — see java/README.md's Lombok section.
plugins {
    id("terrace-config.lombok-conventions")
}

dependencies {
    implementation(project(":terrace-config-annotations"))
    implementation(project(":terrace-config-core"))
    testImplementation(libs.compile.testing)
}
