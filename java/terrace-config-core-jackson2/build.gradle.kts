// The Jackson-2-based codec for the document model in `terrace-config-core` (PR 4 follow-up).
// Exists as its own module — rather than living inside `-core` — so a consumer whose own stack is
// Jackson 3 (e.g. a future Spring Boot 4 app) never has to pull Jackson 2 onto its classpath at
// all: see the sibling `terrace-config-core-jackson3` and java/README.md's "Dual Jackson 2 / 3
// support" section.
description = "The byte-stable JSON codec for the contract model, built on Jackson 2.x " +
    "(`com.fasterxml.jackson.databind`). See java/README.md."

plugins {
    id("terrace-config.java-conventions")
}

dependencies {
    implementation(project(":terrace-config-core"))
    implementation(libs.jackson.databind)
    // jspecify's own guidance, not `compileOnly` — see gradle/libs.versions.toml and
    // terrace-config-annotations/build.gradle.kts for the one deliberate exception to it.
    implementation(libs.jspecify)
}
