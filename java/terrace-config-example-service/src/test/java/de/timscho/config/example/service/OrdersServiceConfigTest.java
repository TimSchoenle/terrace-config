package de.timscho.config.example.service;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import de.timscho.config.loader.LoaderException;
import de.timscho.config.loader.TerraceLoader;

/**
 * The example's own {@link Config}, exercised through {@link TerraceLoader} exactly as {@link
 * OrdersService#main} does — the Java counterpart of {@code tests/example_service.rs}.
 *
 * <p>This module keeps {@link Config} in {@code src/main}, not behind a path trick as the Rust
 * example needs: a Gradle module's test sources already see its own main sources on the same
 * classpath, so there is nothing to share here that is not already shared. What matters is the
 * same thing that matters on the Rust side — these tests load the exact class {@link
 * OrdersService} boots with, not a redeclared copy that could quietly drift from it.
 */
class OrdersServiceConfigTest {

    @TempDir
    Path tmp;

    /** A loader over a config path inside the sandbox, so a stray {@code config.toml} in the working directory can never decide a test. */
    private TerraceLoader loader() {
        return TerraceLoader.of("ORDERS_").reserve("ORDERS_PROFILE").defaultConfigPath(tmp.resolve("config.toml"));
    }

    @Test
    void compiledDefaultsAreEnoughToBoot() {
        Config config = loader().load(Config.class, Map.of());

        assertThat(config.getBindAddr()).isEqualTo("127.0.0.1");
        assertThat(config.getPort()).isEqualTo(8080);
        assertThat(config.getLogLevel()).isEqualTo(LogLevel.INFO);
        assertThat(config.getDatabase().getMaxConnections()).isEqualTo(10);
        assertThat(config.getDatabase().getUrl()).isEqualTo("postgres://localhost/orders_dev");
    }

    @Test
    void aTomlFileOverridesTheListenerAndTheDatabase() throws IOException {
        Files.writeString(tmp.resolve("config.toml"), """
                bind_addr = "0.0.0.0"
                port = 9090

                [database]
                url = "postgres://file/orders"
                max_connections = 25
                """);

        Config config = loader().load(Config.class, Map.of());

        assertThat(config.getBindAddr()).isEqualTo("0.0.0.0");
        assertThat(config.getPort()).isEqualTo(9090);
        assertThat(config.getDatabase().getMaxConnections()).isEqualTo(25);
        assertThat(config.getDatabase().getUrl()).isEqualTo("postgres://file/orders");
    }

    @Test
    void anEnvironmentVariableOutranksTheTomlFile() throws IOException {
        Files.writeString(tmp.resolve("config.toml"), "port = 9090\n");

        Map<String, String> env = Map.of("ORDERS_PORT", "7070");

        Config config = loader().load(Config.class, env);

        assertThat(config.getPort()).isEqualTo(7070);
    }

    @Test
    void theDatabaseUrlCanComeFromAMountedSecret() throws IOException {
        Path secretsDir = tmp.resolve("secrets");
        Files.createDirectory(secretsDir);
        // A trailing newline, which is what a Kubernetes `Secret` key and every editor produce.
        Files.writeString(secretsDir.resolve("database__url"), "postgres://secret/orders\n");

        Map<String, String> env = Map.of("ORDERS_SECRETS_DIR", secretsDir.toString());

        Config config = loader().load(Config.class, env);

        assertThat(config.getDatabase().getUrl()).isEqualTo("postgres://secret/orders");
    }

    @Test
    void theDatabaseUrlCanComeFromFileIndirection() throws IOException {
        Path secretFile = tmp.resolve("db-url.txt");
        Files.writeString(secretFile, "postgres://indirect/orders");

        Map<String, String> env = Map.of("ORDERS_DATABASE__URL_FILE", secretFile.toString());

        Config config = loader().load(Config.class, env);

        assertThat(config.getDatabase().getUrl()).isEqualTo("postgres://indirect/orders");
    }

    @Test
    void anUnrecognisedLogLevelIsRefusedRatherThanSilentlyDefaulted() {
        Map<String, String> env = Map.of("ORDERS_LOG_LEVEL", "verbose");

        assertThatThrownBy(() -> loader().load(Config.class, env)).isInstanceOf(IllegalArgumentException.class);
    }

    @Test
    void theDatabaseUrlSuppliedByBothASecretAndTheEnvironmentIsRefused() throws IOException {
        Path secretsDir = tmp.resolve("secrets");
        Files.createDirectory(secretsDir);
        Files.writeString(secretsDir.resolve("database__url"), "postgres://secret/orders");

        Map<String, String> env = new LinkedHashMap<>();
        env.put("ORDERS_SECRETS_DIR", secretsDir.toString());
        env.put("ORDERS_DATABASE__URL", "postgres://environment/orders");

        assertThatThrownBy(() -> loader().load(Config.class, env))
                .isInstanceOf(LoaderException.class)
                .hasMessageContaining("supplied twice");
    }
}
