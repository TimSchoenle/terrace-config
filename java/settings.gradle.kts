// Aggregator only, with no code of its own. Building it builds every module in dependency order.
// Every module shares the version in gradle.properties, and release-please's `java` package
// releases them together.

plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
}

rootProject.name = "terrace-config-parent"

// The version catalog at gradle/libs.versions.toml is picked up by convention; no explicit
// dependencyResolutionManagement block needed.
include(
    "terrace-config-annotations",
    "terrace-config-core",
    "terrace-config-core-jackson2",
    "terrace-config-core-jackson3",
    "terrace-config-processor",
    "terrace-config-loader",
    "terrace-config-spring-boot",
    "terrace-config-spec-tck",
    "terrace-config-gradle-plugin",
    "terrace-config-example-service",
    "terrace-config-example-spring-service",
)
