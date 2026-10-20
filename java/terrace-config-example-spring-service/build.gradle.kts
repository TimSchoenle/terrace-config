// A worked example of terrace-config-spring-boot: the same "orders" service as
// terrace-config-example-service, this time configured through Spring's own `Binder` rather than
// the vanilla loader. `OrdersPropertiesBindingTest` and
// `OrdersServiceFileIndirectionIntegrationTest` exercise this module's own `OrdersProperties`
// through a real Spring context, so — like the other two examples — it is kept correct by real
// assertions, not merely by compiling. `OrdersProperties` is also `@TerraceConfig`, so the
// `de.timscho.terrace-config` plugin emits `contract.json` from it through `SpringContractMain` at
// build time, and `terraceContractCheck` (part of `check`) fails when the committed copy drifts.
description = "A worked example of terrace-config-spring-boot: the same service configuration " +
    "as terrace-config-example-service, bound through Spring instead. See java/README.md."

plugins {
    application
    id("terrace-config.lombok-conventions")
    id("de.timscho.terrace-config")
}

terraceContract {
    type = "de.timscho.config.example.springservice.OrdersProperties"
    prefix = "ORDERS_"
    appName = "orders-service"
}

dependencies {
    // The `_FILE` indirection `EnvironmentPostProcessor`, `SpringDialect` and
    // `SpringContractProducer` — everything a service takes from this contract beyond plain
    // Spring Boot. `application.yml`/environment-variable binding itself needs no dependency on
    // it at all.
    implementation(project(":terrace-config-spring-boot"))
    implementation(project(":terrace-config-annotations"))
    // The generated `*Descriptor` classes are `terrace-config-core`'s `TypeDescriptor`s.
    implementation(project(":terrace-config-core"))
    // The codec `SpringContractMain` writes the contract with, on the generator's classpath only,
    // so the service itself does not ship it.
    terraceContractGenerator(project(":terrace-config-core-jackson2"))
    compileOnly(project(":terrace-config-processor"))
    annotationProcessor(project(":terrace-config-processor"))
    implementation(libs.spring.boot.starter)
    testImplementation(libs.spring.boot.starter.test)
    // Only for `Database.url`'s `@JsonProperty(access = WRITE_ONLY)`, which Spring's binder ignores
    // and `SpringContractMain`'s Jackson reads when it converts a default-constructed
    // `OrdersProperties` to defaults. `compileOnly`: the annotation is needed to compile, and at
    // build time the generator's own classpath carries it; nothing at runtime reads it. The tests
    // compile against the same classes, so they see it too.
    compileOnly(libs.jackson.databind)
    testCompileOnly(libs.jackson.databind)
}

application {
    mainClass.set("de.timscho.config.example.springservice.OrdersServiceApplication")
}
