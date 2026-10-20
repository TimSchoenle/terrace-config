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

// The `de.timscho.terrace-config` plugin, compiled a second time here from
// terrace-config-gradle-plugin's own sources so the example modules in this same build can apply
// it by id: a plugin a build publishes is not on that build's own plugin classpath, and buildSrc
// is. One set of sources, so the copy the examples run and the copy JitPack publishes cannot
// differ; terrace-config-gradle-plugin stays the module that tests, documents and publishes it.
sourceSets {
    main {
        java.srcDir("../terrace-config-gradle-plugin/src/main/java")
    }
}

dependencies {
    implementation("org.jspecify:jspecify:${libs.versions.jspecify.get()}")
}

gradlePlugin {
    plugins {
        create("terraceConfig") {
            id = "de.timscho.terrace-config"
            implementationClass = "de.timscho.config.gradle.TerraceContractPlugin"
        }
    }
}
