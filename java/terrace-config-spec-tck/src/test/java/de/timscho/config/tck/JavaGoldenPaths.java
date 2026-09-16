package de.timscho.config.tck;

import java.nio.file.Path;
import java.nio.file.Paths;

/**
 * Where this module's own Tier 3 self-conformance corpus lives — the Java-only counterpart of
 * {@link SpecPaths}'s {@code spec/v1/conformance/}. Kept as a resource under this module rather
 * than in {@code spec/v1/}: {@code spec/v1/conformance/} is the one shared, Rust-authored corpus
 * every implementation compares its dialect spellings against (tier 2); a Java golden is a
 * property of this implementation alone, and belongs next to the tests that keep it honest.
 */
final class JavaGoldenPaths {

    private JavaGoldenPaths() {}

    static Path conformanceJavaDir() {
        return Paths.get("").toAbsolutePath().resolve("src/test/resources/conformance-java");
    }

    static Path goldenCase(final String name) {
        return conformanceJavaDir().resolve(name).resolve("contract.json");
    }
}
