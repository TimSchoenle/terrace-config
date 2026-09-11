package de.timscho.config.core.schema;

import static org.assertj.core.api.Assertions.assertThat;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import org.junit.jupiter.api.Test;

import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.TextForm;

class DefaultsTest {

    private static Dialect dialect() {
        return Dialect.builder()
                .prefix("TEST_")
                .nestingSeparator("__")
                .indirectionSuffix("_FILE")
                .build();
    }

    private static Key.KeyBuilder baseKey(String path) {
        return Key.builder().path(path).docs("").textForm(TextForm.TEXT).required(false);
    }

    @Test
    void fills_in_a_top_level_default() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("port").ty("u16").build()))
                .build();

        Schema filled = schema.withDefaultsFromValue(Map.of("port", 8080));

        Key port = filled.getKeys().get(0);
        assertThat(port.getDefaultText()).isEqualTo("8080");
        assertThat(port.getDefaultValue()).isEqualTo(8080);
    }

    @Test
    void fills_in_a_nested_default() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("database.url").ty("String").build()))
                .build();

        Map<String, Object> database = new LinkedHashMap<>();
        database.put("url", "postgres://localhost/app");
        Schema filled = schema.withDefaultsFromValue(Map.of("database", database));

        assertThat(filled.getKeys().get(0).getDefaultText()).isEqualTo("postgres://localhost/app");
    }

    @Test
    void a_required_key_keeps_no_default_even_if_the_value_has_one() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("port").ty("u16").required(true).build()))
                .build();

        Schema filled = schema.withDefaultsFromValue(Map.of("port", 8080));

        assertThat(filled.getKeys().get(0).getDefaultText()).isNull();
    }

    @Test
    void a_secret_key_never_carries_its_real_value() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("token").ty("String").secret(true).build()))
                .build();

        Schema filled = schema.withDefaultsFromValue(Map.of("token", "top-secret"));

        Key token = filled.getKeys().get(0);
        assertThat(token.getDefaultText()).isEqualTo("<redacted>");
        assertThat(token.getDefaultValue()).isNull();
    }

    @Test
    void an_absent_path_leaves_the_key_unchanged() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("missing").ty("String").build()))
                .build();

        Schema filled = schema.withDefaultsFromValue(Map.of());

        assertThat(filled.getKeys().get(0).getDefaultText()).isNull();
    }

    @Test
    void an_empty_string_default_is_rendered_quoted() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("name").ty("String").build()))
                .build();

        Schema filled = schema.withDefaultsFromValue(Map.of("name", ""));

        assertThat(filled.getKeys().get(0).getDefaultText()).isEqualTo("\"\"");
    }

    @Test
    void a_list_default_renders_each_element() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("tags").ty("List<String>").build()))
                .build();

        Schema filled = schema.withDefaultsFromValue(Map.of("tags", List.of("a", "b")));

        assertThat(filled.getKeys().get(0).getDefaultText()).isEqualTo("[a, b]");
    }
}
