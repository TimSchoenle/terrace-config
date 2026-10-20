// The `de.timscho.terrace-config` Gradle plugin: a service's contract emitted from its types at
// build time through `ContractMain`/`SpringContractMain`, stamped and conformed by the
// `terrace-contract` binary, and kept equal to the committed copy. Knows about Gradle and about
// how to run those two entry points, and nothing about either producer's internals: it never
// links against any other module here, so a service's build never has two copies of them.
description = "The de.timscho.terrace-config Gradle plugin: emits, stamps and conforms a " +
    "service's configuration contract at build time. See java/README.md."

// `java-gradle-plugin` before `publish-conventions`: the convention reads which kind of artefact
// this is when it is applied, and publishes a Gradle plugin (with its marker) rather than a library
// only when `java-gradle-plugin` is already there.
plugins {
    `java-gradle-plugin`
    id("terrace-config.java-conventions")
    id("terrace-config.publish-conventions")
}

gradlePlugin {
    plugins {
        create("terraceConfig") {
            id = "de.timscho.terrace-config"
            implementationClass = "de.timscho.config.gradle.TerraceContractPlugin"
            displayName = "terrace-config contract"
            description = project.description
        }
    }
}

dependencies {
    // jspecify's own guidance, not `compileOnly` — see gradle/libs.versions.toml and
    // terrace-config-annotations/build.gradle.kts for the one deliberate exception to it.
    implementation(libs.jspecify)
    testImplementation(gradleTestKit())
}

// Gradle 9 runs on Java 17 and later, so the plugin is compiled for 17 whatever toolchain builds
// it: a build on an older daemon JVM than this repository's 25 can still apply it.
tasks.compileJava {
    options.release = 17
}

tasks.test {
    // The functional tests run `terrace-contract`; see TerraceContractPluginFunctionalTest for the
    // rule when it is missing. Which binary, and whether `CI` is set, decide the result, so both
    // are inputs and a rebuilt binary reruns the suite.
    val explicitBinary = providers.environmentVariable("TERRACE_CONTRACT_BIN").orElse("")
    inputs.property("terraceContractBin", explicitBinary)
    inputs.property("ci", providers.environmentVariable("CI").orElse(""))
    inputs.files(
        explicitBinary.map { path -> if (path.isBlank()) emptyList() else listOf(file(path)) },
        rootProject.file("../cli/target/release/terrace-contract"),
        rootProject.file("../cli/target/debug/terrace-contract"),
    ).withPropertyName("terraceContractBinary").optional()
    // The fixture the stub entry point emits: a corpus document every tier accepts.
    val fixture = rootProject.file("../spec/v1/conformance/minimal/contract.json")
    inputs.file(fixture).withPropertyName("fixture").withPathSensitivity(PathSensitivity.NONE)
    systemProperty("terrace.plugin.fixture", fixture.absolutePath)
    systemProperty("terrace.plugin.repositoryRoot", rootProject.file("..").absolutePath)
}
