// The corpus runner and tier comparator (PR 3). Knows about documents, not about binders — a
// Spring import here would be a design failure. Testable before any producer exists: it validates
// the stored corpus against the meta-schema on its own.
description = "The meta-schema validator and tier comparator checked against spec/v1/. " +
    "See java/README.md."

// Only the plain conventions, not terrace-config.lombok-conventions: this module's classes
// (MetaSchemaValidator, TierComparator, Tier, ConformanceVersion) were written and tested before
// Lombok was wired into the build, and none of them is a bean-shaped model class that Lombok
// would meaningfully shrink — retrofitting it would touch tested code for no behavioural gain.
// New classes here can still opt in later.
plugins {
    id("terrace-config.java-conventions")
}

dependencies {
    implementation(libs.jackson.databind)
    implementation(libs.json.schema.validator)
    // jspecify's own guidance, not `compileOnly` — see gradle/libs.versions.toml and
    // terrace-config-annotations/build.gradle.kts for the one deliberate exception to it.
    implementation(libs.jspecify)

    // Test-only: the fixture types under src/test/.../fixtures/ are real `@TerraceConfig` classes,
    // assembled into a real `Contract` through the real loader and rendered through the real
    // Jackson 2 codec — exactly the pipeline a consumer runs, not a hand-maintained restatement of
    // it. Kept out of `implementation` above: this module's own two classes (MetaSchemaValidator,
    // TierComparator) know nothing about descriptors, loaders or bean-shaped config types, and
    // that stays true for anything published from `main`.
    testImplementation(project(":terrace-config-annotations"))
    testImplementation(project(":terrace-config-core"))
    testImplementation(project(":terrace-config-loader"))
    testImplementation(project(":terrace-config-core-jackson2"))
    testCompileOnly(project(":terrace-config-processor"))
    testAnnotationProcessor(project(":terrace-config-processor"))
}

// Two isolated runtime classpaths for JacksonParityTest, one per Jackson major version. Both
// modules publish a class named `de.timscho.config.core.io.ContractCodec` — deliberately the same
// name, so a consumer never has to know which one it linked against — which means the two can
// never sit on one classloader's classpath together without one shadowing the other. Resolving
// each as its own `Usage.JAVA_RUNTIME` configuration (rather than adding either as a normal test
// dependency) is what lets the test load each into its own `URLClassLoader` and ask both the same
// question without either seeing the other.
val jackson2Runtime: Configuration by configurations.creating {
    isCanBeConsumed = false
    isCanBeResolved = true
    isTransitive = true
    attributes {
        attribute(Usage.USAGE_ATTRIBUTE, objects.named(Usage::class, Usage.JAVA_RUNTIME))
    }
}
val jackson3Runtime: Configuration by configurations.creating {
    isCanBeConsumed = false
    isCanBeResolved = true
    isTransitive = true
    attributes {
        attribute(Usage.USAGE_ATTRIBUTE, objects.named(Usage::class, Usage.JAVA_RUNTIME))
    }
}

dependencies {
    jackson2Runtime(project(":terrace-config-core-jackson2"))
    jackson3Runtime(project(":terrace-config-core-jackson3"))
}

tasks.test {
    // `inputs.files(...)`, not just reading `.asPath` below: a `Configuration` is `Buildable`, so
    // registering it as a task input is what makes Gradle build `terrace-config-core-jackson2`/
    // `-jackson3`'s jars before `test` runs. Reading `.asPath` alone only computes a path string at
    // configuration time -- it asks nothing of the task graph, so without this the jars those
    // paths name may not exist yet the first time this runs.
    inputs.files(jackson2Runtime)
    inputs.files(jackson3Runtime)
    // Read by JacksonParityTest to build its two isolated classloaders. A file collection, not a
    // single jar, since each side also needs `terrace-config-core` and its own Jackson major
    // version's databind jar on the classpath.
    systemProperty("terrace.tck.jackson2Classpath", jackson2Runtime.asPath)
    systemProperty("terrace.tck.jackson3Classpath", jackson3Runtime.asPath)
    // Regenerates java/terrace-config-spec-tck/src/test/resources/conformance-java/<case>/contract.json
    // from what the fixtures render today, instead of asserting against what is checked in. See
    // JavaConformanceTest's class Javadoc for when this is the right thing to run.
    System.getProperty("terrace.spec.bless")?.let { systemProperty("terrace.spec.bless", it) }
    if (System.getenv("TERRACE_JAVA_SPEC_BLESS") != null) {
        systemProperty("terrace.spec.bless", "true")
    }
}

// `./gradlew :terrace-config-spec-tck:blessJavaConformance` — the named entry point the developer
// workflow below documents, rather than making every contributor remember the `-D` spelling.
// Forces the flag on regardless of the invoking command line, then runs the exact same suite `test`
// would, so a blessed golden is proven to satisfy every other check (meta-schema, tier 2 against
// the shared spec corpus) in the same run that wrote it.
tasks.register<Test>("blessJavaConformance") {
    group = "verification"
    description = "Regenerates the Java Tier 3 self-conformance goldens under " +
        "src/test/resources/conformance-java/ from what the fixtures render today."
    dependsOn(tasks.named("testClasses"))
    testClassesDirs = sourceSets.test.get().output.classesDirs
    classpath = sourceSets.test.get().runtimeClasspath
    useJUnitPlatform()
    inputs.files(jackson2Runtime)
    inputs.files(jackson3Runtime)
    systemProperty("terrace.spec.bless", "true")
    systemProperty("terrace.tck.jackson2Classpath", jackson2Runtime.asPath)
    systemProperty("terrace.tck.jackson3Classpath", jackson3Runtime.asPath)
}
