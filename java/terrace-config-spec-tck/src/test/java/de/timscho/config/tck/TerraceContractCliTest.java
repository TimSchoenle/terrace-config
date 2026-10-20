package de.timscho.config.tck;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.io.File;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.concurrent.TimeUnit;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.DisabledOnOs;
import org.junit.jupiter.api.condition.OS;
import org.junit.jupiter.api.io.TempDir;

/**
 * Where {@link TerraceContractCli} finds the binary and what it reports from one, against stub
 * executables in a temporary directory rather than the real binary, so the lookup order, the
 * refusal of a bad {@code TERRACE_CONTRACT_BIN}, the exact command line and the captured output
 * are each checked on their own. The stubs are shell scripts, so this class runs where a shell
 * does.
 */
@DisabledOnOs(OS.WINDOWS)
class TerraceContractCliTest {

    @TempDir
    Path root;

    @Test
    void anExplicitBinaryWinsOverABuiltOne() throws IOException {
        executable("cli/target/release/terrace-contract", "");
        final Path explicit = executable("elsewhere/terrace-contract", "");

        assertThat(TerraceContractCli.locate(this.root, explicit.toString(), null))
                .contains(new TerraceContractCli(explicit));
    }

    @Test
    void anExplicitPathThatIsNotExecutableIsAnErrorNotAReasonToLookElsewhere() throws IOException {
        executable("cli/target/release/terrace-contract", "");
        final Path plain = this.root.resolve("plain");
        Files.writeString(plain, "");

        assertThatThrownBy(() -> TerraceContractCli.locate(this.root, plain.toString(), null))
                .isInstanceOf(IllegalStateException.class)
                .hasMessage("TERRACE_CONTRACT_BIN=" + plain + " is not an executable file");
    }

    @Test
    void aBlankExplicitPathCountsAsUnset() throws IOException {
        final Path release = executable("cli/target/release/terrace-contract", "");

        assertThat(TerraceContractCli.locate(this.root, " ", null)).contains(new TerraceContractCli(release));
    }

    @Test
    void aReleaseBuildBeatsADebugBuildWhichBeatsPath() throws IOException {
        final Path onPath = executable("bin/terrace-contract", "");
        final Path debug = executable("cli/target/debug/terrace-contract", "");

        assertThat(TerraceContractCli.locate(this.root, null, onPath.getParent().toString()))
                .contains(new TerraceContractCli(debug));

        final Path release = executable("cli/target/release/terrace-contract", "");

        assertThat(TerraceContractCli.locate(this.root, null, onPath.getParent().toString()))
                .contains(new TerraceContractCli(release));
    }

    @Test
    void pathIsSearchedInOrderSkippingEmptyEntries() throws IOException {
        final Path first = executable("first/terrace-contract", "");
        final Path second = executable("second/terrace-contract", "");
        final String path = File.pathSeparator
                + this.root.resolve("none")
                + File.pathSeparator
                + first.getParent()
                + File.pathSeparator
                + second.getParent();

        assertThat(TerraceContractCli.locate(this.root, null, path)).contains(new TerraceContractCli(first));
    }

    @Test
    void nothingFoundIsEmptyAndTheMessageNamesEveryPlace() {
        assertThat(TerraceContractCli.locate(this.root, null, null)).isEmpty();
        assertThat(TerraceContractCli.searched(this.root))
                .isEqualTo("TERRACE_CONTRACT_BIN (unset), " + this.root.resolve("cli/target/release/terrace-contract")
                        + ", " + this.root.resolve("cli/target/debug/terrace-contract")
                        + ", and terrace-contract on PATH");
    }

    @Test
    void conformPassesTheTierAndAFileHoldingExactlyTheDocumentThenRemovesIt() throws IOException {
        final Path record = this.root.resolve("record");
        final TerraceContractCli cli =
                new TerraceContractCli(executable("bin/terrace-contract", """
                printf '%%s\\n' "$@" > %1$s
                cat "$4" >> %1$s
                echo 'finding on stdout'
                echo 'finding on stderr' >&2
                exit 1
                """.formatted(record)));

        final TerraceContractCli.Outcome outcome =
                cli.conform(Tier.TIER_2, "{\"a\": 1}".getBytes(StandardCharsets.UTF_8));

        final java.util.List<String> lines = Files.readAllLines(record);
        assertThat(lines.subList(0, 3)).containsExactly("conform", "--tier", "2");
        assertThat(lines.get(4)).isEqualTo("{\"a\": 1}");
        assertThat(Path.of(lines.get(3))).isAbsolute().doesNotExist();
        assertThat(outcome.exitCode()).isEqualTo(1);
        assertThat(outcome.conforms()).isFalse();
        assertThat(outcome.output()).contains("finding on stdout").contains("finding on stderr");
    }

    @Test
    void everyTierIsSpelledAsTheBinaryReadsIt() throws IOException {
        final TerraceContractCli cli = new TerraceContractCli(executable("bin/terrace-contract", "echo \"$3\""));
        final Path contract = this.root.resolve("contract.json");

        assertThat(cli.conform(Tier.TIER_1, contract).output()).isEqualTo("1\n");
        assertThat(cli.conform(Tier.TIER_2, contract).output()).isEqualTo("2\n");
        assertThat(cli.conform(Tier.TIER_3, contract).output()).isEqualTo("3\n");
    }

    @Test
    void verifyDockerfileNamesTheDockerfileAndTheContract() throws IOException {
        final TerraceContractCli cli = new TerraceContractCli(executable("bin/terrace-contract", "echo \"$@\""));

        final TerraceContractCli.Outcome outcome =
                cli.verifyDockerfile(Path.of("contract.json"), Path.of("Dockerfile"));

        assertThat(outcome.conforms()).isTrue();
        assertThat(outcome.output())
                .isEqualTo("image verify --dockerfile " + Path.of("Dockerfile").toAbsolutePath() + " "
                        + Path.of("contract.json").toAbsolutePath() + "\n");
    }

    @Test
    void aBinaryThatCannotBeStartedIsReportedAsAnIoFailure() {
        final TerraceContractCli cli = new TerraceContractCli(this.root.resolve("missing/terrace-contract"));

        assertThatThrownBy(() -> cli.conform(Tier.TIER_1, this.root.resolve("contract.json")))
                .isInstanceOf(UncheckedIOException.class);
    }

    @Test
    void anInterruptedWaitKillsTheBinaryAndKeepsTheInterruptFlag() throws Exception {
        final Path binary = this.executable("terrace-contract", "sleep 30\n");
        final TerraceContractCli cli = new TerraceContractCli(binary);

        Thread.currentThread().interrupt();
        try {
            assertThatThrownBy(() -> cli.conform(Tier.TIER_1, this.root.resolve("contract.json")))
                    .isInstanceOf(IllegalStateException.class)
                    .hasMessageContaining("interrupted while running")
                    .hasCauseInstanceOf(InterruptedException.class);
            assertThat(Thread.currentThread().isInterrupted()).isTrue();
        } finally {
            Thread.interrupted();
        }
        // The shell would sleep for 30s; killed, it is gone well within the bound.
        final List<ProcessHandle> left = ProcessHandle.current()
                .children()
                .filter(child -> child.info().commandLine().orElse("").contains(binary.toString()))
                .toList();
        for (final ProcessHandle child : left) {
            child.onExit().get(10, TimeUnit.SECONDS);
        }
    }

    private Path executable(final String relative, final String script) throws IOException {
        final Path file = this.root.resolve(relative);
        Files.createDirectories(file.getParent());
        Files.writeString(file, "#!/bin/sh\n" + script);
        file.toFile().setExecutable(true);
        return file;
    }
}
