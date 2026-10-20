package de.timscho.config.spring;

import static org.assertj.core.api.Assertions.assertThat;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import de.timscho.config.core.contract.ContractAssembler;
import de.timscho.config.core.contract.ProducerCommandLine;
import de.timscho.config.core.io.ContractCodec;
import de.timscho.config.core.model.App;
import java.io.ByteArrayOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.util.Map;
import org.junit.jupiter.api.Test;

/**
 * {@link SpringContractMain} against a real generated descriptor, without an application context:
 * the command line has to produce exactly what {@link SpringContractProducer} produces in code
 * from the same type, with the defaults a default-constructed instance holds.
 */
class SpringContractMainTest {

    private final ByteArrayOutputStream out = new ByteArrayOutputStream();
    private final ByteArrayOutputStream err = new ByteArrayOutputStream();

    private int run(final String... args) {
        return SpringContractMain.commandLine()
                .run(
                        args,
                        new PrintStream(this.out, true, StandardCharsets.UTF_8),
                        new PrintStream(this.err, true, StandardCharsets.UTF_8));
    }

    @Test
    void emitsWhatTheProducerAssemblesInCodeWithTheTypesOwnDefaults() {
        int status = run("--type", ServiceConfig.class.getName(), "--prefix", "PORTFOLIO_", "--app-name", "portfolio");

        byte[] expected = ContractCodec.write(SpringContractProducer.produce(
                ServiceConfigDescriptor.DESCRIPTOR,
                "PORTFOLIO_",
                SpringDialect.standard(),
                App.builder().name("portfolio").build(),
                ContractAssembler.noExternalSurface(),
                new ObjectMapper().convertValue(new ServiceConfig(), new TypeReference<Map<String, Object>>() {})));
        assertThat(status).as(this.err.toString(StandardCharsets.UTF_8)).isEqualTo(ProducerCommandLine.OK);
        assertThat(this.out.toByteArray()).isEqualTo(expected);
    }

    @Test
    void theDialectOptionsReachTheDocument() {
        int status = run(
                "--type", ServiceConfig.class.getName(),
                "--prefix", "PORTFOLIO_",
                "--app-name", "portfolio",
                "--nesting-separator", "__",
                "--file-suffix", "_PATH");

        assertThat(status).as(this.err.toString(StandardCharsets.UTF_8)).isEqualTo(ProducerCommandLine.OK);
        assertThat(this.out.toString(StandardCharsets.UTF_8))
                .contains("\"nesting_separator\": \"__\"")
                .contains("\"indirection_suffix\": \"_PATH\"")
                .contains("\"loader\": \"spring-boot\"");
    }

    @Test
    void aMissingRequiredOptionIsAUsageError() {
        assertThat(run("--type", ServiceConfig.class.getName(), "--prefix", "PORTFOLIO_"))
                .isEqualTo(ProducerCommandLine.USAGE);
        assertThat(this.err.toString(StandardCharsets.UTF_8)).startsWith("error: `--app-name` is required");
    }
}
