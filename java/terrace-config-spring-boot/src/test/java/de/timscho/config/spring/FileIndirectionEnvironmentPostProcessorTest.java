package de.timscho.config.spring;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import org.springframework.boot.SpringApplication;
import org.springframework.core.env.MapPropertySource;
import org.springframework.core.env.StandardEnvironment;
import org.springframework.mock.env.MockEnvironment;

class FileIndirectionEnvironmentPostProcessorTest {

    @TempDir
    Path tempDir;

    private final FileIndirectionEnvironmentPostProcessor postProcessor = new FileIndirectionEnvironmentPostProcessor();
    private final SpringApplication application = new SpringApplication();

    @Test
    void a_file_suffixed_variable_supplies_the_property_its_target_names() throws IOException {
        Path secret = tempDir.resolve("token");
        Files.writeString(secret, "s3cr3t\n", StandardCharsets.UTF_8);

        MockEnvironment environment = systemEnvironment(Map.of("MYAPP_GITHUB_TOKEN_FILE", secret.toString()));

        postProcessor.postProcessEnvironment(environment, application);

        assertThat(environment.getProperty("myapp.github.token")).isEqualTo("s3cr3t");
    }

    @Test
    void the_trailing_newline_is_trimmed_but_not_other_trailing_whitespace() throws IOException {
        Path secret = tempDir.resolve("token");
        Files.writeString(secret, "s3cr3t \r\n", StandardCharsets.UTF_8);

        MockEnvironment environment = systemEnvironment(Map.of("MYAPP_GITHUB_TOKEN_FILE", secret.toString()));

        postProcessor.postProcessEnvironment(environment, application);

        assertThat(environment.getProperty("myapp.github.token")).isEqualTo("s3cr3t ");
    }

    @Test
    void a_variable_without_the_suffix_is_left_alone() {
        MockEnvironment environment = systemEnvironment(Map.of("MYAPP_GITHUB_TOKEN", "plain-value"));

        postProcessor.postProcessEnvironment(environment, application);

        // No file indirection here, so nothing is added — the raw variable is untouched, still
        // only reachable under its own exact name (relaxed binding is the `Binder`'s job, not
        // this post-processor's, and the test's own `MapPropertySource` stand-in for the real
        // `SystemEnvironmentPropertySource` does not apply it either).
        assertThat(environment.getPropertySources().contains("terraceFileIndirection"))
                .isFalse();
        assertThat(environment.getProperty("MYAPP_GITHUB_TOKEN")).isEqualTo("plain-value");
    }

    @Test
    void no_indirection_variables_means_no_property_source_is_added() {
        MockEnvironment environment = systemEnvironment(Map.of());

        postProcessor.postProcessEnvironment(environment, application);

        assertThat(environment.getPropertySources().contains("terraceFileIndirection"))
                .isFalse();
    }

    @Test
    void a_missing_file_is_refused_naming_the_variable() {
        Path missing = tempDir.resolve("does-not-exist");
        MockEnvironment environment = systemEnvironment(Map.of("MYAPP_GITHUB_TOKEN_FILE", missing.toString()));

        assertThatThrownBy(() -> postProcessor.postProcessEnvironment(environment, application))
                .isInstanceOf(FileIndirectionException.class)
                .hasMessageContaining("MYAPP_GITHUB_TOKEN_FILE");
    }

    @Test
    void a_file_that_is_not_valid_utf8_is_refused_naming_the_variable() throws IOException {
        Path secret = tempDir.resolve("token");
        Files.write(secret, new byte[] {(byte) 0xFF, (byte) 0xFE});

        MockEnvironment environment = systemEnvironment(Map.of("MYAPP_GITHUB_TOKEN_FILE", secret.toString()));

        assertThatThrownBy(() -> postProcessor.postProcessEnvironment(environment, application))
                .isInstanceOf(FileIndirectionException.class)
                .hasMessageContaining("MYAPP_GITHUB_TOKEN_FILE");
    }

    @Test
    void the_file_backed_value_outranks_an_ordinary_property_already_present() throws IOException {
        Path secret = tempDir.resolve("token");
        Files.writeString(secret, "from-file", StandardCharsets.UTF_8);

        MockEnvironment environment = systemEnvironment(Map.of("MYAPP_GITHUB_TOKEN_FILE", secret.toString()));
        environment
                .getPropertySources()
                .addLast(new MapPropertySource(
                        "application.properties", Map.of("myapp.github.token", "from-config-file")));

        postProcessor.postProcessEnvironment(environment, application);

        assertThat(environment.getProperty("myapp.github.token")).isEqualTo("from-file");
    }

    /** A {@link MockEnvironment} carrying {@code entries} under the system-environment property source's own name. */
    private static MockEnvironment systemEnvironment(Map<String, String> entries) {
        MockEnvironment environment = new MockEnvironment();
        Map<String, Object> asObjects = new LinkedHashMap<>(entries);
        environment
                .getPropertySources()
                .addFirst(
                        new MapPropertySource(StandardEnvironment.SYSTEM_ENVIRONMENT_PROPERTY_SOURCE_NAME, asObjects));
        return environment;
    }
}
