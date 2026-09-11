package de.timscho.config.loader;

import static org.assertj.core.api.Assertions.assertThat;

import org.junit.jupiter.api.Test;

import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.LoaderRole;
import de.timscho.config.core.model.LoaderVar;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.TextForm;
import de.timscho.config.core.model.UnreachableReason;

/**
 * Proves {@link TerraceLoader#schema} against a real generated descriptor
 * ({@link AppConfig}/{@code AppConfigDescriptor}), covering a nested struct, a choice enum, a
 * secret, a numeric range, and the reserved-key/indirection spelling rules ported from the Rust
 * crate's {@code Schema::describe_at}.
 */
class TerraceLoaderSchemaTest {

    @Test
    void every_field_becomes_a_key_spelled_in_the_loaders_dialect() {
        Schema schema = TerraceLoader.of("TEST_").schema(AppConfigDescriptor.DESCRIPTOR);

        assertThat(schema.getKeys())
                .extracting(Key::getPath)
                .containsExactlyInAnyOrder(
                        "database.url", "database.port", "level", "token", "retries", "retryCount", "max_retries");

        Key url = keyAt(schema, "database.url");
        assertThat(url.getEnv()).isEqualTo("TEST_DATABASE__URL");
        assertThat(url.getEnvFile()).isEqualTo("TEST_DATABASE__URL_FILE");
        assertThat(url.getSecretsFile()).isEqualTo("database__url");
        assertThat(url.getTextForm()).isEqualTo(TextForm.TEXT);
        assertThat(url.getDocs()).contains("connection string");
        assertThat(url.getUnreachable()).isNull();

        Key port = keyAt(schema, "database.port");
        assertThat(port.getEnv()).isEqualTo("TEST_DATABASE__PORT");
        assertThat(port.getTextForm()).isEqualTo(TextForm.INTEGER);

        Key level = keyAt(schema, "level");
        assertThat(level.getEnv()).isEqualTo("TEST_LEVEL");
        assertThat(level.getValues()).containsExactly("TRACE", "DEBUG", "INFO");
        assertThat(level.getTextForm()).isEqualTo(TextForm.CHOICE);
        assertThat(level.getConstraint()).containsEntry("type", "string");

        Key token = keyAt(schema, "token");
        assertThat(token.isSecret()).isTrue();
        assertThat(token.getEnv()).isEqualTo("TEST_TOKEN");
        assertThat(token.isReserved()).isFalse();

        Key retries = keyAt(schema, "retries");
        assertThat(retries.getTextForm()).isEqualTo(TextForm.INTEGER);
        assertThat(retries.getConstraint()).containsEntry("minimum", 0.0).containsEntry("maximum", 65535.0);

        // `retryLimit` is declared `@JsonProperty("max_retries")`: the key path — and so the
        // environment spelling below — follows that rename rather than the Java field name, the
        // same way `TerraceConfigProcessor.renderEnum` already follows it for an enum constant.
        // `TEST_RETRYLIMIT` genuinely does not reach this field; `TEST_MAX_RETRIES` does.
        Key maxRetries = keyAt(schema, "max_retries");
        assertThat(maxRetries.getEnv()).isEqualTo("TEST_MAX_RETRIES");
    }

    @Test
    void the_loader_variables_are_reported_alongside_the_configs_own_keys() {
        Schema schema = TerraceLoader.of("TEST_").schema(AppConfigDescriptor.DESCRIPTOR);

        assertThat(schema.getLoader())
                .extracting(LoaderVar::getEnv, LoaderVar::getRole)
                .contains(
                        org.assertj.core.groups.Tuple.tuple("TEST_CONFIG", LoaderRole.CONFIG),
                        org.assertj.core.groups.Tuple.tuple("TEST_SECRETS_DIR", LoaderRole.SECRETS_DIR));
    }

    @Test
    void a_reserved_key_has_no_file_backed_spelling() {
        Schema schema = TerraceLoader.of("TEST_").reserve("TEST_TOKEN").schema(AppConfigDescriptor.DESCRIPTOR);

        Key token = keyAt(schema, "token");
        assertThat(token.isReserved()).isTrue();
        assertThat(token.getEnvFile()).isNull();
        assertThat(token.getSecretsFile()).isNull();

        assertThat(schema.getLoader())
                .extracting(LoaderVar::getEnv, LoaderVar::getRole)
                .contains(org.assertj.core.groups.Tuple.tuple("TEST_TOKEN", LoaderRole.RESERVED));
    }

    @Test
    void schema_at_nests_the_type_under_the_given_root() {
        Schema schema = TerraceLoader.of("TEST_").schemaAt(AppConfigDescriptor.DESCRIPTOR, "app");

        assertThat(schema.getKeys()).extracting(Key::getPath).contains("app.database.url", "app.level");
        assertThat(keyAt(schema, "app.level").getEnv()).isEqualTo("TEST_APP__LEVEL");
    }

    @Test
    void a_camel_case_path_the_environment_fold_cannot_reproduce_is_unreachable() {
        Schema schema = TerraceLoader.of("TEST_").schema(AppConfigDescriptor.DESCRIPTOR);

        Key retryCount = keyAt(schema, "retryCount");
        assertThat(retryCount.getEnv()).isNull();
        assertThat(retryCount.getEnvFile()).isNull();
        assertThat(retryCount.getSecretsFile()).isNull();
        assertThat(retryCount.getUnreachable()).isEqualTo(UnreachableReason.UNNAMEABLE);
    }

    private static Key keyAt(Schema schema, String path) {
        return schema.getKeys().stream()
                .filter(key -> key.getPath().equals(path))
                .findFirst()
                .orElseThrow(() -> new AssertionError("no key at " + path));
    }
}
