package de.timscho.config.core.schema;

import static org.assertj.core.api.Assertions.assertThat;

import java.util.List;

import org.junit.jupiter.api.Test;

import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.LoaderRole;
import de.timscho.config.core.model.LoaderVar;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.TextForm;

class MarkdownRendererTest {

    private static Dialect dialect() {
        return Dialect.builder()
                .prefix("TEST_")
                .nestingSeparator("__")
                .indirectionSuffix("_FILE")
                .build();
    }

    private static Key.KeyBuilder baseKey(String path) {
        return Key.builder().path(path).docs("").textForm(TextForm.TEXT);
    }

    @Test
    void a_schema_with_no_loader_variables_renders_only_the_keys_table() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("database.url")
                        .env("TEST_DATABASE__URL")
                        .ty("String")
                        .required(true)
                        .build()))
                .build();

        String markdown = schema.toMarkdown();

        assertThat(markdown).doesNotContain("| Variable | Role | Default | Purpose |");
        assertThat(markdown).contains("| TOML | Type | Environment | Default | Flags | Purpose |");
        assertThat(markdown).contains("`database.url`");
        assertThat(markdown).contains("`TEST_DATABASE__URL`");
        assertThat(markdown).contains("required");
    }

    @Test
    void the_loader_table_leads_when_there_is_one() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .loader(List.of(LoaderVar.builder()
                        .env("TEST_CONFIG")
                        .role(LoaderRole.CONFIG)
                        .docs("Names the TOML layer.")
                        .defaultValue("config.toml")
                        .build()))
                .keys(List.of(baseKey("secret")
                        .env("TEST_SECRET")
                        .ty("String")
                        .secret(true)
                        .build()))
                .build();

        String markdown = schema.toMarkdown();

        assertThat(markdown).startsWith("| Variable | Role | Default | Purpose |");
        assertThat(markdown).contains("`TEST_CONFIG`");
        assertThat(markdown).contains("config");
        assertThat(markdown).contains("`config.toml`");
        // A blank line separates the two tables.
        assertThat(markdown).contains("\n\n| TOML");
        assertThat(markdown).contains("secret");
    }

    @Test
    void a_pipe_and_a_backslash_in_docs_are_escaped() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("k").docs("a | b \\ c").build()))
                .build();

        String markdown = schema.toMarkdownKeys(List.of(Column.DOCS));

        assertThat(markdown).contains("a \\| b \\\\ c");
    }

    @Test
    void choices_render_as_a_pipe_separated_list_of_code_spans() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("level")
                        .ty("LogLevel")
                        .values(List.of("trace", "debug", "info"))
                        .build()))
                .build();

        String markdown = schema.toMarkdownKeys(List.of(Column.TYPE));

        assertThat(markdown).contains("`LogLevel`: `trace` \\| `debug` \\| `info`");
    }

    @Test
    void an_unrequired_key_with_no_default_renders_as_unset() {
        Schema schema = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(baseKey("optional").required(false).build()))
                .build();

        String markdown = schema.toMarkdownKeys(List.of(Column.DEFAULT));

        assertThat(markdown).contains("unset");
    }
}
