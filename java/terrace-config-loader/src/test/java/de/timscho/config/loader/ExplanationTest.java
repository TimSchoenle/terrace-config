package de.timscho.config.loader;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ExplanationTest {

    @TempDir
    Path tmp;

    @Test
    void a_toml_only_value_is_attributed_to_the_toml_file_with_no_shadowing() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Explanation explanation =
                TerraceLoader.of("TEST_").defaultConfigPath(config).explain(Map.of());

        Origin url = explanation.origin("database.url").orElseThrow();
        assertThat(url.effective()).isEqualTo(new Layer.Toml(config));
        assertThat(url.isContested()).isFalse();
        assertThat(explanation.contested()).isEmpty();
    }

    @Test
    void an_environment_override_is_reported_as_shadowing_the_toml_value() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Map<String, String> env = new LinkedHashMap<>();
        env.put("TEST_DATABASE__PORT", "6543");

        Explanation explanation =
                TerraceLoader.of("TEST_").defaultConfigPath(config).explain(env);

        Origin port = explanation.origin("database.port").orElseThrow();
        assertThat(port.effective()).isEqualTo(new Layer.Env("TEST_DATABASE__PORT"));
        assertThat(port.shadowed()).containsExactly(new Layer.Toml(config));
        assertThat(port.isContested()).isTrue();
        assertThat(explanation.contested()).extracting(Origin::key).containsExactly("database.port");
    }

    @Test
    void a_secrets_directory_file_is_attributed_by_its_own_path() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, "");

        Path secretsDir = tmp.resolve("secrets");
        Files.createDirectory(secretsDir);
        Path secretFile = secretsDir.resolve("secret");
        Files.writeString(secretFile, "shh");

        Map<String, String> env = new LinkedHashMap<>();
        env.put("TEST_SECRETS_DIR", secretsDir.toString());

        Explanation explanation =
                TerraceLoader.of("TEST_").defaultConfigPath(config).explain(env);

        Origin secret = explanation.origin("secret").orElseThrow();
        assertThat(secret.effective()).isEqualTo(new Layer.SecretsFile(secretFile));
        assertThat(explanation.secretsDir()).contains(secretsDir);
    }

    @Test
    void file_suffix_indirection_is_attributed_to_the_indirection_variable_and_the_named_file() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, "");

        Path secretFile = tmp.resolve("secret.txt");
        Files.writeString(secretFile, "shh");

        Map<String, String> env = new LinkedHashMap<>();
        env.put("TEST_SECRET_FILE", secretFile.toString());

        Explanation explanation =
                TerraceLoader.of("TEST_").defaultConfigPath(config).explain(env);

        Origin secret = explanation.origin("secret").orElseThrow();
        assertThat(secret.effective()).isEqualTo(new Layer.Indirection("TEST_SECRET_FILE", secretFile));
    }

    @Test
    void a_missing_toml_file_is_reported_as_missing_rather_than_an_error() {
        Explanation explanation = TerraceLoader.of("TEST_")
                .defaultConfigPath(tmp.resolve("does-not-exist.toml"))
                .explain(Map.of());

        assertThat(explanation.fragments()).hasSize(1);
        assertThat(explanation.fragments().getFirst().getValue()).isEqualTo(new Fragment.Missing());
    }

    @Test
    void an_unreadable_toml_file_carries_no_reason_in_the_report() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, "this is not [ valid toml");

        Explanation explanation =
                TerraceLoader.of("TEST_").defaultConfigPath(config).explain(Map.of());

        assertThat(explanation.fragments()).hasSize(1);
        assertThat(explanation.fragments().getFirst().getValue()).isEqualTo(new Fragment.Unreadable());
        assertThat(explanation.toString()).contains("not valid TOML").doesNotContain("this is not");
    }

    @Test
    void the_rendered_report_names_the_prefix_key_count_and_every_layer_header() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                [database]
                url = "postgres://localhost/app"
                """);

        Explanation explanation =
                TerraceLoader.of("TEST_").defaultConfigPath(config).explain(Map.of());

        String report = explanation.toString();
        assertThat(report)
                .startsWith("terrace-config: prefix `TEST_`, 1 key")
                .contains("layers, lowest precedence first:")
                .contains("TOML          TEST_CONFIG unset, default " + config)
                .contains("environment   TEST_* (none)")
                .contains("secrets dir   TEST_SECRETS_DIR unset")
                .contains("indirection   TEST_*_FILE (none)")
                .contains("keys:")
                .contains("database.url  <- TOML " + config)
                .doesNotEndWith("\n");
    }
}
