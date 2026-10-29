// The Spring Boot starter and producer. `producer.loader` is `spring-boot`, target tier 1, with
// the `_` vs `__` divergence documented in java/README.md's `terrace-config-spring-boot` section.
// Ships the missing layers as Spring machinery (an EnvironmentPostProcessor for _FILE indirection)
// rather than describing a dialect nothing implements.
description = "terrace-config-spring: the _FILE indirection layer over Spring's Binder, and " +
    "`SpringContractMain`, the build-time producer of its contract, target tier 1. " +
    "See java/README.md."

plugins {
    id("terrace-config.lombok-conventions")
    id("terrace-config.publish-conventions")
}

// Deliberately no Jackson dependency at runtime, declared or transitive-pinned: at runtime this
// module only ever touches JSON through Spring's own `Binder`/`ObjectMapper`, so it automatically
// follows whichever Jackson major version the app's own Spring Boot BOM resolves (2.x under Boot
// 3, 3.x under Boot 4's `tools.jackson` coordinates). `SpringContractMain` runs at build time and
// needs the Jackson 2 codec, so it compiles against it `compileOnly` and finds it on the
// generator's own classpath (see `SpringContractMain`), never on the service's. No
// `-spring-boot4` sibling is needed; java/README.md's "Dual Jackson 2 / 3 support" section has
// the reasoning.
dependencies {
    implementation(project(":terrace-config-core"))
    implementation(project(":terrace-config-annotations"))
    implementation(libs.spring.boot.starter)
    // jspecify's own guidance, not `compileOnly`; see gradle/libs.versions.toml and
    // terrace-config-annotations/build.gradle.kts for the one deliberate exception to it.
    implementation(libs.jspecify)
    compileOnly(project(":terrace-config-core-jackson2"))
    compileOnly(libs.jackson.databind)
    testImplementation(libs.spring.boot.starter.test)
    testImplementation(project(":terrace-config-core-jackson2"))
    testImplementation(libs.jackson.databind)
    testCompileOnly(project(":terrace-config-processor"))
    testAnnotationProcessor(project(":terrace-config-processor"))
}
