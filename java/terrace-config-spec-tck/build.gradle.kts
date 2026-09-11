// The corpus runner and tier comparator (PR 3). Knows about documents, not about binders — a
// Spring import here would be a design failure. Testable before any producer exists: it validates
// the stored corpus against the meta-schema on its own.
description = "The meta-schema validator and tier comparator checked against spec/v1/. " +
    "See java/README.md."

// Only the plain conventions, not terrace-config.lombok-conventions: this module's classes
// (MetaSchemaValidator, TierComparator, Tier, ConformanceVersion) were written and tested before
// Lombok was wired into the build, and none of them is a bean-shaped model class that Lombok
// would meaningfully shrink — retrofitting it would touch tested code for no behavioural gain.
// New classes here can still opt in later.
plugins {
    id("terrace-config.java-conventions")
}

dependencies {
    implementation(libs.jackson.databind)
    implementation(libs.json.schema.validator)
    // jspecify's own guidance, not `compileOnly` — see gradle/libs.versions.toml and
    // terrace-config-annotations/build.gradle.kts for the one deliberate exception to it.
    implementation(libs.jspecify)
}
