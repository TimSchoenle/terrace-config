package de.timscho.config.loader;

import static org.assertj.core.api.Assertions.assertThat;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import de.timscho.config.core.contract.ConfigurationType;
import de.timscho.config.core.contract.ContractAssembler;
import de.timscho.config.core.contract.ProducerCommandLine;
import de.timscho.config.core.contract.ProducerIdentity;
import de.timscho.config.core.io.ContractCodec;
import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Schema;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/**
 * {@link ContractMain} against a real generated descriptor: the command line has to produce
 * exactly the bytes the same loader settings produce in code, or a service's build would publish a
 * document its own types do not describe.
 */
class ContractMainTest {

    private static final ObjectMapper MAPPER = new ObjectMapper();

    private final ByteArrayOutputStream out = new ByteArrayOutputStream();
    private final ByteArrayOutputStream err = new ByteArrayOutputStream();

    private int run(final String... args) {
        return ContractMain.commandLine()
                .run(
                        args,
                        new PrintStream(this.out, true, StandardCharsets.UTF_8),
                        new PrintStream(this.err, true, StandardCharsets.UTF_8));
    }

    @Test
    void emitsWhatTheSameLoaderSettingsAssembleInCode(@TempDir final Path dir) throws IOException {
        Path target = dir.resolve("contract.json");

        int status = run(
                "--type", AppConfig.class.getName(),
                "--prefix", "TEST_",
                "--app-name", "test-service",
                "--reserve", "TEST_PROFILE",
                "--out", target.toString());

        Schema schema = TerraceLoader.of("TEST_")
                .reserve("TEST_PROFILE")
                .schema(AppConfigDescriptor.DESCRIPTOR)
                .withDefaultsFromValue(
                        MAPPER.convertValue(new AppConfig(), new TypeReference<Map<String, Object>>() {}));
        byte[] expected = ContractCodec.write(ContractAssembler.assemble(
                schema, App.builder().name("test-service").build(), ProducerIdentity.forLoader("terrace-java")));
        assertThat(status).as(this.err.toString(StandardCharsets.UTF_8)).isEqualTo(ProducerCommandLine.OK);
        assertThat(Files.readAllBytes(target)).isEqualTo(expected);
    }

    @Test
    void everyLoaderSettingReachesTheDocument() throws IOException {
        int status = run(
                "--type", AppConfig.class.getName(),
                "--prefix", "TEST_",
                "--app-name", "test-service",
                "--nesting-separator", "___",
                "--file-suffix", "_PATH",
                "--config-var", "TEST_TOML",
                "--secrets-dir-var", "TEST_SECRETS",
                "--default-config-path", "/etc/test.toml");

        assertThat(status).as(this.err.toString(StandardCharsets.UTF_8)).isEqualTo(ProducerCommandLine.OK);
        JsonNode document = MAPPER.readTree(this.out.toByteArray());
        JsonNode dialect = document.path("schema").path("dialect");
        assertThat(dialect.path("nesting_separator").asText()).isEqualTo("___");
        assertThat(dialect.path("indirection_suffix").asText()).isEqualTo("_PATH");
        JsonNode loader = document.path("schema").path("loader");
        assertThat(loader.get(0).path("env").asText()).isEqualTo("TEST_TOML");
        assertThat(loader.get(0).path("default").asText()).isEqualTo("/etc/test.toml");
        assertThat(loader.get(1).path("env").asText()).isEqualTo("TEST_SECRETS");
        assertThat(document.path("producer").path("loader").asText()).isEqualTo("terrace-java");
    }

    @Test
    void findsTheGeneratedDescriptorOfANestedTypeByItsBinaryName() {
        ConfigurationType nested =
                ConfigurationType.named(AppConfig.Database.class.getName(), ContractMainTest.class.getClassLoader());

        assertThat(nested.descriptor()).isSameAs(AppConfig$DatabaseDescriptor.DESCRIPTOR);
    }

    @Test
    void aTypeWithoutADescriptorIsAUsageError() {
        int status = run("--type", ContractMainTest.class.getName(), "--prefix", "TEST_", "--app-name", "x");

        assertThat(status).isEqualTo(ProducerCommandLine.USAGE);
        assertThat(this.err.toString(StandardCharsets.UTF_8)).contains("ContractMainTestDescriptor was not found");
        assertThat(this.out.size()).isZero();
    }

    @Test
    void aMissingRequiredOptionIsAUsageError() {
        assertThat(run("--type", AppConfig.class.getName())).isEqualTo(ProducerCommandLine.USAGE);
        assertThat(this.err.toString(StandardCharsets.UTF_8)).startsWith("error: `--prefix` is required");
    }
}
