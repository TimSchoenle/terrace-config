# Publishing

The eight published Java modules are served by JitPack, built from a pushed tag on request. This
page covers which modules publish, where their coordinates come from, and how a consumer resolves
the libraries and the Gradle plugin.

## What is published

Eight of the eleven modules apply `terrace-config.publish-conventions`:

- `terrace-config-annotations`
- `terrace-config-core`
- `terrace-config-core-jackson2`
- `terrace-config-core-jackson3`
- `terrace-config-processor`
- `terrace-config-loader`
- `terrace-config-spring-boot`
- `terrace-config-gradle-plugin`

`terrace-config-spec-tck` is a test tool for `spec/v1/conformance/`, and the two `-example-*`
modules are worked examples meant to be read, so none of the three is published.

All eight share the one `version` in `java/gradle.properties`, so release-please's `java` package
releases the plugin together with the libraries. Its tags are `terrace-config-java-v<version>`.

## Coordinates

Nothing is set per module. `group` and `version` come from `java/gradle.properties`, and the
plugin's default `artifactId` is the Gradle project name, which already matches. A module states
only its own `pom.name` and `pom.description`, taken from the `description` its
`build.gradle.kts` declares. Everything common to a release (licence, developer, SCM URL) lives
once in `terrace-config.publish-conventions`.

## Why JitPack

Maven Central is not configured: there is no Sonatype account, no GPG key and no signing.
[JitPack](https://jitpack.io) needs none of them. It builds from a pushed tag the first time
someone resolves it and caches the result, so the tag is the release and there is no publish step
to run. `terrace-config.publish-conventions.gradle.kts` says what to add
(`publishToMavenCentral`, `signAllPublications` and a release workflow) if Central publishing is
wanted.

[`jitpack.yml`](../../jitpack.yml) sits at the repository root, where JitPack looks for it. It
tells JitPack that the Gradle root is `java/` and to run `publishToMavenLocal` without the test
suite, since this repository's CI already gates on it. JitPack serves whatever lands in
`~/.m2/repository`, and that task writes the jars, sources jars, Javadoc jars and POMs of all eight
modules there with no credentials. The JDK on JitPack's image only has to run the wrapper: the
foojay toolchain resolver in `settings.gradle.kts` downloads the JDK `javaVersion` names.

## Depending on a module

A consumer adds the repository and depends on a tag, or on a branch name or short commit SHA for
something unreleased:

```kotlin
repositories {
    maven { url = uri("https://jitpack.io") }
}

dependencies {
    // One module: the group is `com.github.<user>.<repo>`, the artifact is the Gradle module name.
    implementation("com.github.TimSchoenle.terrace-config:terrace-config-loader:<tag>")
    // Every published module at once: the group is `com.github.<user>`, the artifact is the repo.
    implementation("com.github.TimSchoenle:terrace-config:<tag>")
}
```

Only the eight modules above resolve this way.

## Applying the Gradle plugin

JitPack serves every artefact under its own `com.github.TimSchoenle.terrace-config` group, not
under the `de.timscho.terrace-config` group a plugin marker is looked up by. A consumer's
`settings.gradle.kts` maps the id to the module:

```kotlin
pluginManagement {
    repositories {
        gradlePluginPortal()
        maven { url = uri("https://jitpack.io") }
    }
    resolutionStrategy {
        eachPlugin {
            if (requested.id.id == "de.timscho.terrace-config") {
                useModule("com.github.TimSchoenle.terrace-config:terrace-config-gradle-plugin:${requested.version}")
            }
        }
    }
}
```

The build then applies it with `id("de.timscho.terrace-config") version "<tag>"`.
