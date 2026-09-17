// Applied by every module under java/ (directly, or transitively through
// terrace-config.lombok-conventions). Replaces what used to be the root java/build.gradle.kts's
// `subprojects { ... }` block: same Java 25 toolchain, UTF-8 encoding, JUnit 5 + AssertJ test
// dependencies, and the JetBrains annotations vocabulary, for every module, just expressed once
// here instead of via cross-project configuration. See java/README.md's "buildSrc" section.
import org.gradle.api.artifacts.VersionCatalogsExtension
import org.gradle.api.plugins.quality.Checkstyle

plugins {
    java
    checkstyle
    id("com.diffplug.spotless")
}

// Precompiled script plugins can't use the type-safe `libs.xxx` accessors (Gradle has to assume
// the plugin might be applied to a project with a different catalog shape), only this
// lower-level, string-keyed lookup — see java/README.md's "buildSrc" section.
val libs = extensions.getByType(VersionCatalogsExtension::class.java).named("libs")

repositories {
    mavenCentral()
    // Only for the checkstyle-library-ruleset coordinate below - see the checkstyle block.
    maven { url = uri("https://jitpack.io") }
}

// Holds the ruleset jar so `resources.text.fromArchiveEntry` (in the checkstyle block below) can
// pull checkstyle.xml out of it; the plain `checkstyle` configuration Gradle's checkstyle plugin
// already provides is for the tool's own runtime classpath, not for this.
val checkstyleConfig: Configuration by configurations.creating

java {
    toolchain {
        languageVersion.set(JavaLanguageVersion.of(25))
    }
}

tasks.withType<JavaCompile> {
    options.encoding = "UTF-8"
}

tasks.withType<Javadoc> {
    options.encoding = "UTF-8"
}

// Every module gets JUnit 5 and AssertJ in test scope; nothing here reaches main.
dependencies {
    testImplementation(platform(libs.findLibrary("junit-bom").get()))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testImplementation(libs.findLibrary("assertj-core").get())
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

// `@Contract`, `@Unmodifiable`, `@Blocking`/`@NonBlocking`, `@VisibleForTesting` and the rest of
// the IntelliJ inspection vocabulary jspecify doesn't cover -- see the catalog entry in
// gradle/libs.versions.toml for why every module, including terrace-config-annotations, takes
// this uniformly and only as `compileOnly`: CLASS retention means it is never a runtime
// requirement, so unlike jspecify (see each module's own dependencies block) there is no
// per-module decision to make here.
dependencies {
    compileOnly(libs.findLibrary("jetbrains-annotations").get())
    testCompileOnly(libs.findLibrary("jetbrains-annotations").get())
}

tasks.test {
    useJUnitPlatform()
}

checkstyle {
    toolVersion = libs.findVersion("checkstyle").get().requiredVersion
    // checkstyleMain runs the "library" tier ruleset straight from its canonical JitPack
    // publication (com.github.TimSchoenle.actions:checkstyle-library, built from TimSchoenle/
    // actions:configs/checkstyle/library — see the catalog comment on checkstyle-library-ruleset
    // for why the coordinate isn't the de.timscho one that repo's own README documents) instead of
    // a locally vendored copy, so a rule change there only needs a version bump in
    // gradle/libs.versions.toml to take effect — no hand-synced duplicate file to keep in step,
    // and unlike fetching straight from `main`, the version is pinned and reviewable like any
    // other dependency bump.
    //
    // That jar already bundles checkstyle-suppressions.xml, custom-suppressions.xml and
    // lombok-xpath-suppressions.xml alongside checkstyle.xml, with every `${config_loc}/...`
    // reference inside checkstyle.xml pre-rewritten to `classpath:/...` at publish time. So unlike
    // a `configDirectory`-based setup, nothing needs vendoring here: Checkstyle resolves those
    // suppression files against the jar itself, provided the jar is on the same classpath as the
    // checkstyle tool at runtime — see the `checkstyle` configuration dependency below.
    config = resources.text.fromArchiveEntry(checkstyleConfig, "checkstyle.xml")
    isIgnoreFailures = false
    maxWarnings = 0
}

dependencies {
    checkstyleConfig(libs.findLibrary("checkstyle-library-ruleset").get())
    // The ruleset jar must also sit on the `checkstyle` configuration itself (the tool's own
    // runtime classpath), not just `checkstyleConfig` above - that's what makes the `classpath:/...`
    // suppression-file lookups inside checkstyle.xml resolve at all. But the checkstyle plugin only
    // auto-adds `com.puppycrawl.tools:checkstyle:$toolVersion` to that configuration via
    // Configuration.defaultDependencies, which backs off the moment anything is added to the
    // configuration explicitly - so adding the ruleset jar here silently drops the tool itself
    // (ClassNotFoundException: CheckstyleAntTask) unless it's added back explicitly too.
    checkstyle(libs.findLibrary("checkstyle-library-ruleset").get())
    checkstyle("com.puppycrawl.tools:checkstyle:${libs.findVersion("checkstyle").get().requiredVersion}")
}

tasks.named<Checkstyle>("checkstyleTest") {
    configFile = rootProject.file("config/checkstyle/checkstyle-test.xml")
}

spotless {
    java {
        palantirJavaFormat(libs.findVersion("palantir-java-format").get().requiredVersion)
        formatAnnotations()
        // One flat, alphabetically-sorted block with no blank-line separation — matches
        // checkstyleMain's JitPack-consumed "library" tier ImportOrder (groups="/.*/", so every
        // import falls into the same single group; see terrace-config.java-conventions'
        // checkstyle block). Previously grouped java/javax/de.timscho with blank lines between
        // them, which that check's single-group expectation doesn't recognize as valid separation.
        importOrder("")
        removeUnusedImports()
        trimTrailingWhitespace()
        endWithNewline()
    }
}
