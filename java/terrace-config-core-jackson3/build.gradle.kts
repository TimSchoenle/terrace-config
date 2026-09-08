// The Jackson-3-based codec for the document model in `terrace-config-core` (PR 4 follow-up).
// Mirrors `terrace-config-core-jackson2` field for field, built on `tools.jackson.*` instead —
// see that module's own comment and java/README.md's "Dual Jackson 2 / 3 support" section.
description = "The byte-stable JSON codec for the contract model, built on Jackson 3.x " +
    "(`tools.jackson.databind`). See java/README.md."

plugins {
    id("terrace-config.java-conventions")
}

dependencies {
    implementation(project(":terrace-config-core"))
    implementation(libs.jackson.databind.v3)
}
