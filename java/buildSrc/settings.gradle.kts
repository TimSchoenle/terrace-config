// buildSrc is its own separate Gradle build, so it doesn't automatically see the `libs` version
// catalog declared for the main build's modules under java/gradle/libs.versions.toml. Pointing
// buildSrc's own catalog at that same file (rather than duplicating versions here) keeps exactly
// one place — java/gradle/libs.versions.toml — as the source of truth for every version, and
// gives the convention plugins below the identical type-safe `libs.versions.*` accessors that
// every module's own build.gradle.kts already uses.
dependencyResolutionManagement {
    versionCatalogs {
        create("libs") {
            from(files("../gradle/libs.versions.toml"))
        }
    }
}
