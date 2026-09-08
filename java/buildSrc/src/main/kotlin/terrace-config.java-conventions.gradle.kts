// Applied by every module under java/ (directly, or transitively through
// terrace-config.lombok-conventions). Replaces what used to be the root java/build.gradle.kts's
// `subprojects { ... }` block: same Java 21 toolchain, UTF-8 encoding, and JUnit 5 + AssertJ test
// dependencies for every module, just expressed once here instead of via cross-project
// configuration. See java/README.md's "buildSrc" section.
import org.gradle.api.artifacts.VersionCatalogsExtension

plugins {
    java
}

// Precompiled script plugins can't use the type-safe `libs.xxx` accessors (Gradle has to assume
// the plugin might be applied to a project with a different catalog shape), only this
// lower-level, string-keyed lookup — see java/README.md's "buildSrc" section.
val libs = extensions.getByType(VersionCatalogsExtension::class.java).named("libs")

repositories {
    mavenCentral()
}

java {
    toolchain {
        languageVersion.set(JavaLanguageVersion.of(21))
    }
}

tasks.withType<JavaCompile> {
    options.encoding = "UTF-8"
}

tasks.withType<Javadoc> {
    options.encoding = "UTF-8"
}

// Every module gets JUnit 5 and AssertJ in test scope; nothing here reaches main.
dependencies {
    testImplementation(platform(libs.findLibrary("junit-bom").get()))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testImplementation(libs.findLibrary("assertj-core").get())
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

tasks.test {
    useJUnitPlatform()
}
