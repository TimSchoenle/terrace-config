package de.timscho.config.loader;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class LoadWatchedTest {

    record Database(String url, int port) {
    }

    record Config(Database database, String secret) {
    }

    @TempDir
    Path tmp;

    @Test
    void watch_paths_include_the_toml_files_own_directory() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                secret = "compiled-in"

                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Loaded<Config> loaded = TerraceLoader.of("TEST_")
                .defaultConfigPath(config)
                .loadWatched(Config.class, Map.of());

        assertThat(loaded.value().secret()).isEqualTo("compiled-in");
        assertThat(loaded.sources().watchPaths()).contains(config.toAbsolutePath().getParent());
    }

    @Test
    void watch_paths_include_the_secrets_directory() throws IOException {
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

        Loaded<Config> loaded = TerraceLoader.of("TEST_")
                .defaultConfigPath(config)
                .loadWatched(Config.class, env);

        assertThat(loaded.sources().watchPaths()).contains(secretsDir.toAbsolutePath());
    }

    @Test
    void watch_paths_include_a_file_indirection_targets_parent() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Path secretFile = tmp.resolve("secret.txt");
        Files.writeString(secretFile, "from-file-indirection");

        Map<String, String> env = new LinkedHashMap<>();
        env.put("TEST_SECRET_FILE", secretFile.toString());

        Loaded<Config> loaded = TerraceLoader.of("TEST_")
                .defaultConfigPath(config)
                .loadWatched(Config.class, env);

        assertThat(loaded.sources().watchPaths()).contains(secretFile.toAbsolutePath().getParent());
    }

    @Test
    void an_unchanged_environment_is_not_a_change() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                secret = "compiled-in"

                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        TerraceLoader loader = TerraceLoader.of("TEST_").defaultConfigPath(config);
        Loaded<Config> first = loader.loadWatched(Config.class, Map.of());
        Loaded<Config> again = loader.loadWatched(Config.class, Map.of());

        assertThat(first.sources().differsFrom(again.sources())).isFalse();
    }

    @Test
    void a_changed_value_is_a_change() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                secret = "compiled-in"

                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        TerraceLoader loader = TerraceLoader.of("TEST_").defaultConfigPath(config);
        Loaded<Config> before = loader.loadWatched(Config.class, Map.of());

        Files.writeString(config, """
                secret = "compiled-in"

                [database]
                url = "postgres://localhost/app"
                port = 6543
                """);
        Loaded<Config> after = loader.loadWatched(Config.class, Map.of());

        assertThat(before.sources().differsFrom(after.sources())).isTrue();
    }

    @Test
    void a_nan_value_compares_equal_to_itself() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                secret = "compiled-in"

                [database]
                url = "postgres://localhost/app"
                port = 5432
                timeout = nan
                """);

        record DatabaseWithTimeout(String url, int port, double timeout) {
        }
        record ConfigWithTimeout(DatabaseWithTimeout database, String secret) {
        }

        TerraceLoader loader = TerraceLoader.of("TEST_").defaultConfigPath(config);
        Loaded<ConfigWithTimeout> first = loader.loadWatched(ConfigWithTimeout.class, Map.of());
        Loaded<ConfigWithTimeout> again = loader.loadWatched(ConfigWithTimeout.class, Map.of());

        assertThat(Double.isNaN(first.value().database().timeout())).isTrue();
        assertThat(first.sources().differsFrom(again.sources())).isFalse();
    }

    @Test
    void the_fingerprint_never_appears_in_tostring() throws IOException {
        Path config = tmp.resolve("config.toml");
        Files.writeString(config, """
                secret = "top-secret-value"

                [database]
                url = "postgres://localhost/app"
                port = 5432
                """);

        Loaded<Config> loaded = TerraceLoader.of("TEST_")
                .defaultConfigPath(config)
                .loadWatched(Config.class, Map.of());

        assertThat(loaded.sources().toString()).doesNotContain("top-secret-value");
    }
}
