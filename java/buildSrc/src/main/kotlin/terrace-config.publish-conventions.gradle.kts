// Applied by every module that ships a publishable artefact: -annotations, -core,
// -core-jackson2, -core-jackson3, -processor, -loader, -spring-boot. Deliberately not applied to
// -spec-tck or either example module -- those are internal tooling and demo services, not
// something a consumer's build should ever depend on. See java/README.md's "Publishing to
// JitPack" section for the one place this is actually wired up right now.
//
// Maven Central is deliberately NOT configured here for now -- no `publishToMavenCentral(...)`,
// no `signAllPublications()`, and so no Central Portal account, user token or GPG key is needed
// for anything in this file to work. What's left (the POM metadata below, plus the javadoc/
// sources jars `configure(JavaLibrary(...))` sets up) is exactly what `publishToMavenLocal` needs
// and JitPack's own build already exercises -- see jitpack.yml at the repository root. Revisit
// this file (reintroducing `publishToMavenCentral`/`signAllPublications`, the four secrets, and
// `.github/workflows/release-java.yml`) if/when Central publishing is wanted again.
//
// GAV coordinates are never set here: group and version come from the `allprojects {}` block in
// the root java/build.gradle.kts, and the plugin defaults artifactId to the Gradle project name,
// which already matches (`terrace-config-core`, ...) -- so there is no `coordinates(...)` call to
// keep in step with settings.gradle.kts. Only what is identical for every published module --
// licence, developer, SCM -- lives here; `pom.name`/`pom.description` are filled from the
// module's own `description` property, already declared per module in its own build.gradle.kts
// for exactly this purpose.
import com.vanniktech.maven.publish.JavaLibrary
import com.vanniktech.maven.publish.JavadocJar
import com.vanniktech.maven.publish.SourcesJar

plugins {
    id("com.vanniktech.maven.publish")
}

mavenPublishing {
    // Explicit rather than relying on auto-detection: the plugin only auto-detects Android/Kotlin
    // project types, and every module here applies the plain `java` plugin (see
    // terrace-config.java-conventions), not `java-library`. A real `-javadoc.jar`, not
    // `JavadocJar.Empty()`, since `javadoc.destinationDir` (wired in java-conventions) already
    // gives every module real API docs to publish.
    configure(JavaLibrary(javadocJar = JavadocJar.Javadoc(), sourcesJar = SourcesJar.Sources()))

    pom {
        name.set(project.name)
        // Lazy, not `project.description` directly: `plugins {}` (this convention plugin
        // included) applies before the rest of a module's own build.gradle.kts runs, so at the
        // point this line executes the module's own `description = "..."` assignment further
        // down that file has not happened yet. A provider defers the read to when the POM is
        // actually generated, well after configuration finishes.
        description.set(project.provider { project.description })
        url.set("https://github.com/TimSchoenle/terrace-config")

        licenses {
            license {
                name.set("MIT License")
                url.set("https://github.com/TimSchoenle/terrace-config/blob/main/LICENSE")
                distribution.set("repo")
            }
        }

        developers {
            developer {
                id.set("TimSchoenle")
                name.set("Tim Schönle")
                email.set("tim.schoenle@posteo.de")
                url.set("https://github.com/TimSchoenle")
            }
        }

        scm {
            url.set("https://github.com/TimSchoenle/terrace-config")
            connection.set("scm:git:git://github.com/TimSchoenle/terrace-config.git")
            developerConnection.set("scm:git:ssh://git@github.com/TimSchoenle/terrace-config.git")
        }
    }
}
