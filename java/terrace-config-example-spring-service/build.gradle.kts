// A worked example of terrace-config-spring-boot: the same "orders" service as
// terrace-config-example-service, this time configured through Spring's own `Binder` rather than
// the vanilla loader. `OrdersPropertiesBindingTest` and
// `OrdersServiceFileIndirectionIntegrationTest` exercise this module's own `OrdersProperties`
// through a real Spring context, so — like the other two examples — it is kept correct by real
// assertions, not merely by compiling. `OrdersProperties` is also `@TerraceConfig`, so
// `ContractGenerator` can build a real `Contract` through `SpringContractProducer` and
// `ContractTest` checks `contract.json` fresh on every run.
description = "A worked example of terrace-config-spring-boot: the same service configuration " +
    "as terrace-config-example-service, bound through Spring instead. See java/README.md."

plugins {
    application
    id("terrace-config.lombok-conventions")
}

dependencies {
    // The `_FILE` indirection `EnvironmentPostProcessor`, `SpringDialect` and
    // `SpringContractProducer` — everything a service takes from this contract beyond plain
    // Spring Boot. `application.yml`/environment-variable binding itself needs no dependency on
    // it at all.
    implementation(project(":terrace-config-spring-boot"))
    implementation(project(":terrace-config-annotations"))
    implementation(project(":terrace-config-core"))
    implementation(project(":terrace-config-core-jackson2"))
    compileOnly(project(":terrace-config-processor"))
    annotationProcessor(project(":terrace-config-processor"))
    implementation(libs.spring.boot.starter)
    testImplementation(libs.spring.boot.starter.test)
    // `spring-boot-starter` alone pulls in no Jackson: that only arrives with `-starter-json` (via
    // `-starter-web`), which this plain, non-web service never depends on. `ContractGenerator`
    // needs an `ObjectMapper` of its own to convert a default-constructed `OrdersProperties` to
    // the nested map `SpringContractProducer.produce`'s defaults parameter expects — the same
    // reason `terrace-config-example-service` declares this explicitly rather than relying on a
    // transitive `implementation` dependency two modules away.
    implementation(libs.jackson.databind)
}

application {
    mainClass.set("de.timscho.config.example.springservice.OrdersServiceApplication")
}
