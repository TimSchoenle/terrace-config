package de.timscho.config.tck;

import java.io.File;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Optional;
import java.util.concurrent.TimeUnit;
import org.jetbrains.annotations.Blocking;
import org.jspecify.annotations.Nullable;

/**
 * The real {@code terrace-contract} binary, run as a subprocess: the one implementation of {@code
 * conform}, so Java's documents are held to the same checks every other producer's are, rather
 * than to a Java restatement of them.
 *
 * <p>Found, in order, at:
 *
 * <ol>
 *   <li>{@code TERRACE_CONTRACT_BIN}, which must name an executable file when set: an explicit
 *       setting that points nowhere is a mistake to report, never a reason to look elsewhere;
 *   <li>{@code cli/target/release/terrace-contract}, then {@code cli/target/debug/terrace-contract},
 *       under the repository root, which is where {@code cargo build} in {@code cli/} leaves it;
 *   <li>the first {@code terrace-contract} on {@code PATH}.
 * </ol>
 *
 * @param executable the binary
 */
record TerraceContractCli(Path executable) {

    /** The variable naming the binary explicitly. */
    static final String BIN_VARIABLE = "TERRACE_CONTRACT_BIN";

    /** Generous for a document of a few kilobytes; only a hung process reaches it. */
    private static final long TIMEOUT_SECONDS = 60;

    private static final String NAME_ON_UNIX = "terrace-contract";

    private static final String NAME = isWindows() ? NAME_ON_UNIX + ".exe" : NAME_ON_UNIX;

    /**
     * The binary, or empty when none is found anywhere, read from this process's environment.
     *
     * @param repositoryRoot the root {@code cli/target/} is resolved against
     * @throws IllegalStateException when {@value #BIN_VARIABLE} is set but names no executable file
     */
    static Optional<TerraceContractCli> locate(final Path repositoryRoot) {
        return locate(repositoryRoot, System.getenv(BIN_VARIABLE), System.getenv("PATH"));
    }

    /**
     * The binary, or empty when none is found anywhere.
     *
     * @param repositoryRoot the root {@code cli/target/} is resolved against
     * @param explicit       the value of {@value #BIN_VARIABLE}, or {@code null} when unset
     * @param path           the value of {@code PATH}, or {@code null} when unset
     * @throws IllegalStateException when {@code explicit} is set but names no executable file
     */
    static Optional<TerraceContractCli> locate(
            final Path repositoryRoot, final @Nullable String explicit, final @Nullable String path) {
        if (explicit != null && !explicit.isBlank()) {
            final Path named = Path.of(explicit).toAbsolutePath();
            if (!isExecutableFile(named)) {
                throw new IllegalStateException(BIN_VARIABLE + "=" + explicit + " is not an executable file");
            }
            return Optional.of(new TerraceContractCli(named));
        }
        final List<Path> candidates = new ArrayList<>();
        candidates.add(repositoryRoot.resolve("cli/target/release").resolve(NAME));
        candidates.add(repositoryRoot.resolve("cli/target/debug").resolve(NAME));
        candidates.addAll(onPath(path));
        for (final Path candidate : candidates) {
            if (isExecutableFile(candidate)) {
                return Optional.of(new TerraceContractCli(candidate));
            }
        }
        return Optional.empty();
    }

    /** {@value #NAME_ON_UNIX} (or its {@code .exe}) in each {@code PATH} directory, in order. */
    private static List<Path> onPath(final @Nullable String path) {
        final List<Path> found = new ArrayList<>();
        if (path == null) {
            return found;
        }
        for (final String directory : path.split(File.pathSeparator)) {
            if (!directory.isEmpty()) {
                found.add(Path.of(directory).resolve(NAME));
            }
        }
        return found;
    }

    private static boolean isExecutableFile(final Path path) {
        return Files.isRegularFile(path) && Files.isExecutable(path);
    }

    /** Where {@link #locate} looked, for a message saying why nothing was found. */
    static String searched(final Path repositoryRoot) {
        return BIN_VARIABLE + " (unset), "
                + repositoryRoot.resolve("cli/target/release").resolve(NAME) + ", "
                + repositoryRoot.resolve("cli/target/debug").resolve(NAME) + ", and " + NAME + " on PATH";
    }

    /**
     * Runs {@code terrace-contract conform --tier <tier>} over a document.
     *
     * @param tier     the tier to hold it to
     * @param document the document's bytes, exactly as the producer wrote them
     * @return the exit status and everything the binary printed
     */
    @Blocking
    Outcome conform(final Tier tier, final byte[] document) {
        final Path file;
        try {
            file = Files.createTempFile("terrace-tck-", ".json");
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
        try {
            Files.write(file, document);
            return this.conform(tier, file);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        } finally {
            try {
                Files.deleteIfExists(file);
            } catch (IOException ignored) {
                // A temp file left behind is harmless; failing the check over it would not be.
            }
        }
    }

    /**
     * Runs {@code terrace-contract conform --tier <tier>} over a document on disk.
     *
     * @param tier     the tier to hold it to
     * @param contract the document
     * @return the exit status and everything the binary printed
     */
    @Blocking
    Outcome conform(final Tier tier, final Path contract) {
        return this.run(List.of(
                "conform",
                "--tier",
                tierArgument(tier),
                contract.toAbsolutePath().toString()));
    }

    /**
     * Runs {@code terrace-contract image verify --dockerfile <dockerfile>} over a document: whether
     * the label block committed in a Dockerfile is the one the contract renders.
     *
     * @param contract   the document the image carries
     * @param dockerfile the Dockerfile that builds the image
     * @return the exit status and everything the binary printed
     */
    @Blocking
    Outcome verifyDockerfile(final Path contract, final Path dockerfile) {
        return this.run(List.of(
                "image",
                "verify",
                "--dockerfile",
                dockerfile.toAbsolutePath().toString(),
                contract.toAbsolutePath().toString()));
    }

    private static boolean finished(final Process process, final List<String> command) {
        try {
            return process.waitFor(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        } catch (InterruptedException e) {
            // Nobody is waiting for the result any more, so the binary must not outlive the wait.
            process.destroyForcibly();
            Thread.currentThread().interrupt();
            throw new IllegalStateException("interrupted while running " + String.join(" ", command), e);
        }
    }

    private Outcome run(final List<String> arguments) {
        final List<String> command = new ArrayList<>();
        command.add(this.executable.toString());
        command.addAll(arguments);
        final Path output;
        try {
            output = Files.createTempFile("terrace-tck-", ".out");
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
        try {
            // Output to a file rather than a pipe: nothing has to drain it while the process runs,
            // so a chatty failure cannot block the binary on a full pipe buffer.
            final Process process = new ProcessBuilder(command)
                    .redirectErrorStream(true)
                    .redirectOutput(output.toFile())
                    .redirectInput(ProcessBuilder.Redirect.from(nullDevice()))
                    .start();
            if (!finished(process, command)) {
                process.destroyForcibly();
                throw new IllegalStateException(
                        String.join(" ", command) + " did not finish within " + TIMEOUT_SECONDS + "s");
            }
            return new Outcome(process.exitValue(), Files.readString(output, StandardCharsets.UTF_8));
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        } finally {
            try {
                Files.deleteIfExists(output);
            } catch (IOException ignored) {
                // As above: a leftover temp file must not turn a result into a failure.
            }
        }
    }

    private static String tierArgument(final Tier tier) {
        return switch (tier) {
            case TIER_1 -> "1";
            case TIER_2 -> "2";
            case TIER_3 -> "3";
        };
    }

    private static File nullDevice() {
        return new File(isWindows() ? "NUL" : "/dev/null");
    }

    private static boolean isWindows() {
        final @Nullable String os = System.getProperty("os.name");
        return os != null && os.toLowerCase(Locale.ROOT).startsWith("windows");
    }

    /**
     * What one {@code conform} run reported.
     *
     * @param exitCode {@code 0} conforms, {@code 1} findings, {@code 2} the document could not be
     *                 read
     * @param output   standard output and standard error, interleaved as printed
     */
    record Outcome(int exitCode, String output) {

        /** Whether the document met the tier. */
        boolean conforms() {
            return this.exitCode == 0;
        }
    }
}
