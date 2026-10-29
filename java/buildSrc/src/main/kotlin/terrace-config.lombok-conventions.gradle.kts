// Applied by the modules with Lombok-built classes: -core, -loader, -spring-boot and both
// examples. Bundles terrace-config.java-conventions plus io.freefair.lombok, so a module's own
// build.gradle.kts says `id("terrace-config.lombok-conventions")` instead of applying both and
// repeating the `lombok { version.set(...) }` line each time. See java/docs/BUILD.md for which
// modules opt out and why.
import org.gradle.api.artifacts.VersionCatalogsExtension

plugins {
    id("terrace-config.java-conventions")
    id("io.freefair.lombok")
}

val libs = extensions.getByType(VersionCatalogsExtension::class.java).named("libs")

lombok {
    version.set(libs.findVersion("lombok").get().requiredVersion)
}
