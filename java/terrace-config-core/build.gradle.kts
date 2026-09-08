// The document model and everything true of a contract regardless of loader (PR 4). Knows about
// documents, not about binders — no Spring, no reflection over user types. Depended on by
// -loader, -spring-boot and -spec-tck; depends on nothing that knows about either.
description = "The envelope model, the eight refusals and the renderings — not started yet (PR 4). " +
    "See java/README.md."

// Applying the plugin (rather than hand-rolled `compileOnly`/`annotationProcessor` lines) wires
// main *and* test sources, and points `javadoc` at delomboked sources automatically — see
// java/README.md's Lombok section and the root java/lombok.config.
plugins {
    id("terrace-config.lombok-conventions")
}

// Jackson here is purely an internal implementation detail of this module's own JSON rendering —
// never a type on any public API — so it stays pinned to 2.x regardless of which Jackson major
// version a consuming service's own stack (e.g. a future Spring Boot 4 app) resolves elsewhere.
// Jackson 3 renamed its Maven coordinates (`tools.jackson:*`), so the two cannot even collide on
// a classpath. See docs/migration-progress.md, "Lombok and the Jackson 2 vs. 3 split".
dependencies {
    implementation(libs.jackson.databind)
}
