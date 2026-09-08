// Hosts the convention plugins under src/main/kotlin/ (terrace-config.java-conventions,
// terrace-config.lombok-conventions) that every module's own build.gradle.kts applies, instead of
// each one repeating the same `java`/toolchain/test-framework/Lombok wiring — see
// java/README.md's "buildSrc" section for why.
plugins {
    `kotlin-dsl`
}

repositories {
    gradlePluginPortal()
    mavenCentral()
}

dependencies {
    // The plugin artefact backing `id("io.freefair.lombok")` in terrace-config.lombok-conventions
    // below. Version comes from the same catalog every module's own dependencies use — see
    // settings.gradle.kts in this directory for how buildSrc reaches that file.
    implementation("io.freefair.gradle:lombok-plugin:${libs.versions.lombok.plugin.get()}")
}
