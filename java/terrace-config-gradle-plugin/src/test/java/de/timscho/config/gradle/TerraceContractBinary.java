package de.timscho.config.gradle;

import static org.junit.jupiter.api.Assertions.fail;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import java.io.File;

/**
 * The {@code terrace-contract} binary a functional test hands the build under test, always through
 * {@code TERRACE_CONTRACT_BIN} in that build's environment.
 *
 * <p>Found at {@code TERRACE_CONTRACT_BIN}, then this repository's own {@code
 * cli/target/release/terrace-contract} and {@code cli/target/debug/terrace-contract}, then {@code
 * PATH}. The {@code cli/target} step is this repository's convenience and lives here, in its tests,
 * never in the plugin: the published plugin runs only a binary its build named.
 *
 * <p><b>The rule for a missing binary:</b> under {@code CI=true} it fails the test, and anywhere
 * else it skips the test through a JUnit assumption. This is the same rule the TCK's conform tests
 * follow, for the same reason: CI must not go green by skipping the check, and a contributor
 * without a Rust toolchain must still be able to run the Java build.
 */
final class TerraceContractBinary {

    private TerraceContractBinary() {}

    /** The binary; fails or skips the calling test, per the rule above, when there is none. */
    static File require() {
        final File root = repositoryRoot();
        final String osName = System.getProperty("os.name", "");
        final String name = osName.toLowerCase(java.util.Locale.ROOT).startsWith("windows")
                ? "terrace-contract.exe"
                : "terrace-contract";
        final String explicit = System.getenv(ExecutableLocator.BIN_VARIABLE);
        if (explicit != null && !explicit.isBlank()) {
            return ExecutableLocator.locate(explicit, new File(root, "java"), null, osName);
        }
        for (final String profile : new String[] {"release", "debug"}) {
            final File built = new File(new File(new File(new File(root, "cli"), "target"), profile), name);
            if (built.isFile() && built.canExecute()) {
                return built;
            }
        }
        final File onPath = ExecutableLocator.locate(null, new File(root, "java"), System.getenv("PATH"), osName);
        if (onPath != null) {
            return onPath;
        }
        final String message = "terrace-contract was not found at " + ExecutableLocator.BIN_VARIABLE
                + " (unset), " + root + "/cli/target/{release,debug}/" + name + ", or on PATH. Build it with"
                + " `cargo build --release` in cli/, or set " + ExecutableLocator.BIN_VARIABLE + " to its path.";
        if ("true".equalsIgnoreCase(System.getenv("CI"))) {
            return fail(message + " CI=true, so this fails rather than skips.");
        }
        assumeTrue(false, message + " Skipped, since CI is not set.");
        throw new AssertionError("unreachable: a false assumption aborts the test");
    }

    /** This repository's root, as the build that runs these tests passes it. */
    static File repositoryRoot() {
        return new File(System.getProperty("terrace.plugin.repositoryRoot"));
    }
}
