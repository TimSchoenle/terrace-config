package de.timscho.config.tck;

import static org.junit.jupiter.api.Assertions.fail;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import java.util.Optional;

/**
 * The {@code terrace-contract} binary a test conforms documents with, or the reason it cannot.
 *
 * <p><b>The rule for a missing binary:</b> under {@code CI=true} it fails the test, and anywhere
 * else it skips the test through a JUnit assumption. Failing in CI is what keeps a pipeline that
 * forgot to build the binary from going green by skipping the very check these tests exist for;
 * skipping locally is what lets a contributor without a Rust toolchain still run the Java build.
 * A {@value TerraceContractCli#BIN_VARIABLE} that is set but names no executable file fails
 * everywhere: it is an explicit instruction, and silently looking elsewhere would hide the mistake.
 */
final class TerraceContractBinary {

    private TerraceContractBinary() {}

    /** The binary; fails or skips the calling test, per the rule above, when there is none. */
    static TerraceContractCli require() {
        final Optional<TerraceContractCli> found = TerraceContractCli.locate(SpecPaths.repositoryRoot());
        if (found.isPresent()) {
            return found.get();
        }
        final String message = "terrace-contract was not found. Looked at "
                + TerraceContractCli.searched(SpecPaths.repositoryRoot())
                + ". Build it with `cargo build --release` in cli/, or set "
                + TerraceContractCli.BIN_VARIABLE + " to its path.";
        if ("true".equalsIgnoreCase(System.getenv("CI"))) {
            return fail(message + " CI=true, so this fails rather than skips.");
        }
        assumeTrue(false, message + " Skipped, since CI is not set.");
        throw new AssertionError("unreachable: a false assumption aborts the test");
    }
}
