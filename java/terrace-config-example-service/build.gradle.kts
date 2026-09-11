// A worked example: a small "orders" service loading its configuration through
// terrace-config-loader, mirroring rust/examples/service/ field for field so the two can be read
// side by side as one story about what this contract looks like in each language.
// `OrdersServiceConfigTest` exercises this module's own `Config` class through `TerraceLoader`,
// so — like the Rust example — it is kept correct by a real assertion, not merely by compiling.
// `Config` also carries the `@TerraceConfig` annotations `terrace-config-processor` reads, so
// `--contract` (see `OrdersService`) and `ContractTest` render a real `Contract` from the exact
// same class, the same way `contract.json` follows `examples/service/config.rs` on the Rust side.
description = "A worked example of terrace-config-loader: a small service configuration, " +
    "exercised by its own tests. See java/README.md."

plugins {
    application
    id("terrace-config.lombok-conventions")
}

dependencies {
    implementation(project(":terrace-config-loader"))
    implementation(project(":terrace-config-annotations"))
    implementation(project(":terrace-config-core"))
    implementation(project(":terrace-config-core-jackson2"))
    compileOnly(project(":terrace-config-processor"))
    annotationProcessor(project(":terrace-config-processor"))
    // Only for `@JsonProperty`, to spell a multi-word key the same snake_case way in every layer
    // (a TOML file, an environment variable, a mounted secret) despite Java's camelCase fields —
    // and, since the fix in `FieldResolver`, the same name `terrace-config-processor` reads for
    // the generated descriptor's own key path. `-loader` already depends on this internally as an
    // implementation detail, so it is not exposed transitively — this module needs its own
    // declaration to compile against it.
    implementation(libs.jackson.databind)
}

application {
    mainClass.set("de.timscho.config.example.service.OrdersService")
}
