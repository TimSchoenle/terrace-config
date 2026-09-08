// The Spring Boot starter and producer (PR 7). `producer.loader` is `spring-boot`, target tier 1
// with the `_` vs `__` divergence documented in this module's own README once it exists. Ships the
// missing layers as Spring machinery (an EnvironmentPostProcessor for _FILE indirection) rather
// than describing a dialect nothing implements.
description = "terrace-config-spring: starter + producer over Spring's Binder, target tier 1 — " +
    "not started yet (PR 7). See java/README.md."

plugins {
    id("terrace-config.lombok-conventions")
}

// Deliberately no direct Jackson dependency, declared or transitive-pinned: this module only ever
// touches JSON through Spring's own `Binder`/`ObjectMapper`, so it automatically follows whichever
// Jackson major version the app's own Spring Boot BOM resolves (2.x under Boot 3, 3.x under Boot
// 4's `tools.jackson` coordinates). If Boot 4 support is needed before Boot 3 support is dropped,
// ship it as a sibling `terrace-config-spring-boot4` module rather than branching this one — see
// docs/migration-progress.md, "Lombok and the Jackson 2 vs. 3 split".
dependencies {
    implementation(project(":terrace-config-core"))
    implementation(project(":terrace-config-annotations"))
    implementation(libs.spring.boot.starter)
    testImplementation(libs.spring.boot.starter.test)
}
