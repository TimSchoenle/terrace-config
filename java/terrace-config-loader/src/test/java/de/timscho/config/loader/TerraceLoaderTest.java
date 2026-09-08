package de.timscho.config.loader;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class TerraceLoaderTest {

    record Database(String url, int port) {
    }

    record Config(Database database, String secret) {
    }

    @TempDir
    Path tmp;

    @Test
    void a_toml_file_alone_supplies_every_value() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                secret = "compiled-in"

                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Config loaded = TerraceLoader.of("TEST_")
                .defaultConfigPath(config)
                .load(Config.class, Map.of());

        assertThat(loaded.secret()).isEqualTo("compiled-in");
        assertThat(loaded.database().url()).isEqualTo("postgres://localhost/app");
        assertThat(loaded.database().port()).isEqualTo(5432);
    }

    @Test
    void an_environment_variable_overrides_the_toml_value() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                secret = "compiled-in"

                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Map<String, String> env = new LinkedHashMap<>();
        env.put("TEST_DATABASE__PORT", "6543");

        Config loaded = TerraceLoader.of("TEST_")
                .defaultConfigPath(config)
                .load(Config.class, env);

        assertThat(loaded.database().port()).isEqualTo(6543);
        assertThat(loaded.database().url()).isEqualTo("postgres://localhost/app");
    }

    @Test
    void a_secrets_directory_file_supplies_a_key() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Path secretsDir = tmp.resolve("secrets");
        Files.createDirectory(secretsDir);
        Files.writeString(secretsDir.resolve("secret"), "from-secrets-dir\n");

        Map<String, String> env = new LinkedHashMap<>();
        env.put("TEST_SECRETS_DIR", secretsDir.toString());

        Config loaded = TerraceLoader.of("TEST_")
                .defaultConfigPath(config)
                .load(Config.class, env);

        assertThat(loaded.secret()).isEqualTo("from-secrets-dir");
    }

    @Test
    void file_suffix_indirection_supplies_a_key() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Path secretFile = tmp.resolve("secret.txt");
        Files.writeString(secretFile, "from-file-indirection\r\n");

        Map<String, String> env = new LinkedHashMap<>();
        env.put("TEST_SECRET_FILE", secretFile.toString());

        Config loaded = TerraceLoader.of("TEST_")
                .defaultConfigPath(config)
                .load(Config.class, env);

        assertThat(loaded.secret()).isEqualTo("from-file-indirection");
    }

    @Test
    void a_key_supplied_by_the_environment_and_a_secrets_file_is_rejected() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Path secretsDir = tmp.resolve("secrets");
        Files.createDirectory(secretsDir);
        Files.writeString(secretsDir.resolve("secret"), "from-secrets-dir");

        Map<String, String> env = new LinkedHashMap<>();
        env.put("TEST_SECRETS_DIR", secretsDir.toString());
        env.put("TEST_SECRET", "from-env");

        assertThatThrownBy(() -> TerraceLoader.of("TEST_")
                .defaultConfigPath(config)
                .load(Config.class, env))
                .isInstanceOf(LoaderException.class)
                .hasMessageContaining("supplied twice");
    }

    @Test
    void last_wins_resolves_a_shadowed_key_instead_of_refusing_it() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Path secretsDir = tmp.resolve("secrets");
        Files.createDirectory(secretsDir);
        Files.writeString(secretsDir.resolve("secret"), "from-secrets-dir");

        Map<String, String> env = new LinkedHashMap<>();
        env.put("TEST_SECRETS_DIR", secretsDir.toString());
        env.put("TEST_SECRET", "from-env");

        Config loaded = TerraceLoader.of("TEST_")
                .defaultConfigPath(config)
                .shadowPolicy(ShadowPolicy.LAST_WINS)
                .load(Config.class, env);

        // The secrets directory is merged on top of the plain environment layer.
        assertThat(loaded.secret()).isEqualTo("from-secrets-dir");
    }

    @Test
    void a_reserved_key_cannot_be_supplied_by_a_secrets_file() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                secret = "x"

                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Path secretsDir = tmp.resolve("secrets");
        Files.createDirectory(secretsDir);
        Files.writeString(secretsDir.resolve("profile"), "prod");

        Map<String, String> env = new LinkedHashMap<>();
        env.put("TEST_SECRETS_DIR", secretsDir.toString());

        assertThatThrownBy(() -> TerraceLoader.of("TEST_")
                .defaultConfigPath(config)
                .reserve("TEST_PROFILE")
                .load(Config.class, env))
                .isInstanceOf(LoaderException.class)
                .hasMessageContaining("TEST_PROFILE");
    }

    @Test
    void a_missing_toml_file_is_not_an_error() {
        Map<String, String> env = new LinkedHashMap<>();
        env.put("TEST_DATABASE__URL", "postgres://localhost/app");
        env.put("TEST_DATABASE__PORT", "5432");
        env.put("TEST_SECRET", "value");

        Config loaded = TerraceLoader.of("TEST_")
                .defaultConfigPath(tmp.resolve("does-not-exist.toml"))
                .load(Config.class, env);

        assertThat(loaded.database().url()).isEqualTo("postgres://localhost/app");
    }
}
