package de.timscho.config.core.schema;

import static org.assertj.core.api.Assertions.assertThat;

import java.util.List;
import java.util.Map;

import org.junit.jupiter.api.Test;

import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.LoaderRole;
import de.timscho.config.core.model.LoaderVar;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.TextForm;

class TomlExampleRendererTest {

    private static Dialect dialect() {
        return Dialect.builder().prefix("TEST_").nestingSeparator("__").indirectionSuffix("_FILE").build();
    }

    private static Key.KeyBuilder baseKey(String path) {
        return Key.builder()
                .path(path)
                .docs("")
                .textForm(TextForm.TEXT);
    }

    @Test
    void a_required_key_with_no_default_is_left_uncommented() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("database.url")
                        .env("TEST_DATABASE__URL")
                        .ty("String")
                        .required(true)
                        .build()))
                .build();

        String example = schema.toTomlExampleWith(TomlExampleOptions.defaults().withHeader(false));

        assertThat(example).contains("[database]");
        assertThat(example).contains("url = \"<value>\"");
        assertThat(example).doesNotContain("# url = ");
        assertThat(example).contains("Required: nothing loads until this key is supplied.");
    }

    @Test
    void a_key_with_a_default_is_commented_out_and_shows_the_default() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("port")
                        .env("TEST_PORT")
                        .ty("u16")
                        .defaultText("8080")
                        .defaultValue(8080)
                        .required(false)
                        .build()))
                .build();

        String example = schema.toTomlExampleWith(TomlExampleOptions.defaults().withHeader(false));

        assertThat(example).contains("# port = 8080");
    }

    @Test
    void a_secret_key_is_never_written_with_its_real_default() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("token")
                        .env("TEST_TOKEN")
                        .ty("String")
                        .secret(true)
                        .defaultText("<redacted>")
                        .build()))
                .build();

        String example = schema.toTomlExampleWith(TomlExampleOptions.defaults().withHeader(false));

        assertThat(example).contains("token = \"<secret>\"");
        assertThat(example).contains("Secret: the value below is a placeholder.");
    }

    @Test
    void the_preamble_names_the_loader_variables() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .loader(List.of(LoaderVar.builder()
                        .env("TEST_CONFIG")
                        .role(LoaderRole.CONFIG)
                        .docs("Names the TOML layer.")
                        .defaultValue("config.toml")
                        .build()))
                .keys(List.of())
                .build();

        String example = schema.toTomlExample();

        assertThat(example).contains("TEST_CONFIG -- config, default `config.toml`");
    }

    @Test
    void a_string_is_quoted_and_escaped() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("path")
                        .ty("String")
                        .defaultValue("C:\\logs")
                        .required(false)
                        .build()))
                .build();

        String example = schema.toTomlExampleWith(TomlExampleOptions.defaults().withHeader(false));

        assertThat(example).contains("path = \"C:\\\\logs\"");
    }

    @Test
    void an_unrequired_boolean_key_with_no_default_gets_a_typed_placeholder() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("enabled")
                        .ty("bool")
                        .required(false)
                        .constraint(Map.of("type", "boolean"))
                        .build()))
                .build();

        String example = schema.toTomlExampleWith(TomlExampleOptions.defaults().withHeader(false));

        assertThat(example).contains("enabled = false");
    }

    @Test
    void the_generated_file_actually_parses_as_toml() throws java.io.IOException {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(
                        baseKey("database.url").ty("String").required(true).build(),
                        baseKey("database.port").ty("u16").defaultValue(5432).defaultText("5432").build(),
                        baseKey("name").ty("String").defaultValue("weird \"name\"").defaultText("weird \"name\"").build()))
                .build();

        String example = schema.toTomlExample();

        org.tomlj.TomlParseResult result = org.tomlj.Toml.parse(uncommentEverything(example));
        assertThat(result.hasErrors())
                .withFailMessage(() -> "errors: " + result.errors())
                .isFalse();
    }

    /** Uncomments every commented assignment, so the example's own defaults can be checked as real TOML. */
    private static String uncommentEverything(String example) {
        StringBuilder out = new StringBuilder();
        for (String line : example.split("\n", -1)) {
            if (line.startsWith("# ") && line.substring(2).contains(" = ")) {
                out.append(line.substring(2));
            } else if (line.startsWith("#")) {
                // A plain comment line: drop it.
                continue;
            } else {
                out.append(line);
            }
            out.append('\n');
        }
        return out.toString();
    }
}
