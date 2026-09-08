// Applied by the four modules with (or planned to have) bean-shaped classes: -core, -processor,
// -loader, -spring-boot. Bundles terrace-config.java-conventions plus io.freefair.lombok, so a
// module's own build.gradle.kts just says `id("terrace-config.lombok-conventions")` instead of
// applying both and repeating the `lombok { version.set(...) }` line each time. See
// java/README.md's "Lombok" and "buildSrc" sections for which modules opt out and why.
import org.gradle.api.artifacts.VersionCatalogsExtension

plugins {
    id("terrace-config.java-conventions")
    id("io.freefair.lombok")
}

val libs = extensions.getByType(VersionCatalogsExtension::class.java).named("libs")

lombok {
    version.set(libs.findVersion("lombok").get().requiredVersion)
}
