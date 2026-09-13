// Hosts the convention plugins under src/main/kotlin/ (terrace-config.java-conventions,
// terrace-config.lombok-conventions, terrace-config.publish-conventions) that every module's own
// build.gradle.kts applies, instead of each one repeating the same
// `java`/toolchain/test-framework/Lombok/publishing wiring — see java/README.md's "buildSrc"
// section for why.
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
    // The plugin artefact backing `id("com.diffplug.spotless")` in terrace-config.java-conventions
    // below, same reasoning.
    implementation("com.diffplug.spotless:spotless-plugin-gradle:${libs.versions.spotless.plugin.get()}")
    // The plugin artefact backing `id("com.vanniktech.maven.publish")` in
    // terrace-config.publish-conventions below. Wraps `maven-publish` + `signing` with the
    // Maven Central Portal upload API, so a publishable module's own build.gradle.kts states only
    // its coordinates and description, not a hand-rolled `publications {}`/`repositories {}` pair.
    implementation("com.vanniktech:gradle-maven-publish-plugin:${libs.versions.vanniktech.publish.plugin.get()}")
}
