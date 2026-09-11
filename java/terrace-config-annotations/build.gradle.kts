// The annotation vocabulary a service's own types carry — @TerraceConfig and friends. Depends on
// nothing but the JDK at runtime: a service annotating its configuration types needs these on its
// compile classpath and nothing else, not the document model or the processor that reads them.
description = "@TerraceConfig and the rest of the annotation vocabulary read by " +
    "terrace-config-processor. No dependency beyond the JDK reaches a consumer's runtime or " +
    "compile classpath, by design."

// Only the plain conventions (Java toolchain, JUnit/AssertJ, JetBrains annotations — see
// terrace-config.java-conventions.gradle.kts), not terrace-config.lombok-conventions: annotation
// types (`@interface`) can't carry Lombok annotations at all, and adding the plugin would put
// `lombok` (a compile-time-only jar, admittedly) on this module's classpath for no reason —
// undermining the "nothing beyond the JDK reaches a consumer" guarantee this module exists to
// make. See java/README.md's Lombok section.
plugins {
    id("terrace-config.java-conventions")
}

// jspecify's own guidance (see gradle/libs.versions.toml) is `implementation`/`api`, not
// `compileOnly`, precisely so a consumer's own null-checking tooling can resolve the annotation
// too. That guidance is for ordinary libraries; this module's whole reason to exist is that
// depending on it costs nothing beyond the JDK on a consumer's classpath (see the description
// above), so `compileOnly` here is a deliberate, narrow exception: none of this module's public
// surface is `@interface` element types anyway (an annotation element can't be null), so the only
// things `@Nullable`/`@NullMarked` can describe are this module's few ordinary helper types, and
// losing jspecify off a consumer's classpath costs them nothing they'd otherwise use.
dependencies {
    compileOnly(libs.jspecify)
    testCompileOnly(libs.jspecify)
}
