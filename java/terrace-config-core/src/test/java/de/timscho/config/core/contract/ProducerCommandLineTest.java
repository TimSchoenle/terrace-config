package de.timscho.config.core.contract;

import static org.assertj.core.api.Assertions.assertThat;

import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Schema;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Set;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/** The exit statuses and the {@code --out} handling every producer entry point shares. */
class ProducerCommandLineTest {

    private static final String USAGE = "Usage: test --prefix <PREFIX_>\n";

    /** Renders the prefix alone, so a test can tell which document was written. */
    private static final ProducerCommandLine COMMAND_LINE = new ProducerCommandLine(
            USAGE,
            Set.of("prefix"),
            Set.of(),
            arguments -> contract(arguments.required("prefix")),
            contract -> contract.getSchema().getDialect().getPrefix().getBytes(StandardCharsets.UTF_8));

    private final ByteArrayOutputStream out = new ByteArrayOutputStream();
    private final ByteArrayOutputStream err = new ByteArrayOutputStream();

    private static Contract contract(final String prefix) {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(Dialect.builder()
                        .prefix(prefix)
                        .nestingSeparator("__")
                        .indirectionSuffix("_FILE")
                        .build())
                .keys(List.of())
                .build();
        return ContractAssembler.assemble(
                schema, App.builder().name("test").build(), ProducerIdentity.forLoader("terrace-java"));
    }

    private int run(final String... args) {
        return COMMAND_LINE.run(
                args,
                new PrintStream(this.out, true, StandardCharsets.UTF_8),
                new PrintStream(this.err, true, StandardCharsets.UTF_8));
    }

    @Test
    void writesTheDocumentToStandardOutputByDefault() {
        assertThat(run("--prefix", "APP_")).isEqualTo(ProducerCommandLine.OK);
        assertThat(this.out.toString(StandardCharsets.UTF_8)).isEqualTo("APP_");
        assertThat(this.err.size()).isZero();
    }

    @Test
    void writesTheDocumentToOutCreatingItsDirectory(@TempDir final Path dir) throws IOException {
        Path target = dir.resolve("nested/contract.json");

        assertThat(run("--prefix", "APP_", "--out", target.toString())).isEqualTo(ProducerCommandLine.OK);

        assertThat(Files.readString(target)).isEqualTo("APP_");
        assertThat(this.out.size()).isZero();
    }

    @Test
    void aRefusalIsStatusOneAndNamesTheRefusal() {
        // An empty prefix is the one refusal a contract can reach with no keys at all.
        ProducerCommandLine refusing =
                new ProducerCommandLine(USAGE, Set.of(), Set.of(), arguments -> contract(""), contract -> new byte[0]);

        int status = refusing.run(
                new String[0],
                new PrintStream(this.out, true, StandardCharsets.UTF_8),
                new PrintStream(this.err, true, StandardCharsets.UTF_8));

        assertThat(status).isEqualTo(ProducerCommandLine.REFUSED);
        assertThat(this.err.toString(StandardCharsets.UTF_8)).startsWith("error: refusing to emit this contract: ");
        assertThat(this.out.size()).isZero();
    }

    @Test
    void aUsageErrorIsStatusTwoAndPrintsTheUsage() {
        assertThat(run("--typo", "x")).isEqualTo(ProducerCommandLine.USAGE);
        assertThat(this.err.toString(StandardCharsets.UTF_8))
                .startsWith("error: unknown option `--typo`")
                .endsWith(USAGE);
        assertThat(this.out.size()).isZero();
    }

    @Test
    void helpPrintsTheUsageAndSucceeds() {
        assertThat(run("--help")).isEqualTo(ProducerCommandLine.OK);
        assertThat(this.out.toString(StandardCharsets.UTF_8)).isEqualTo(USAGE);
    }
}
