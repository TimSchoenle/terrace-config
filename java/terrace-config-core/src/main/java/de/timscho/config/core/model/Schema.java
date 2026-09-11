package de.timscho.config.core.model;

import java.util.List;
import java.util.Map;

import com.fasterxml.jackson.annotation.JsonIgnore;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import lombok.Builder;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;

import de.timscho.config.core.schema.Column;
import de.timscho.config.core.schema.Defaults;
import de.timscho.config.core.schema.JsonSchemaOptions;
import de.timscho.config.core.schema.JsonSchemaRenderer;
import de.timscho.config.core.schema.MarkdownRenderer;
import de.timscho.config.core.schema.TomlExampleOptions;
import de.timscho.config.core.schema.TomlExampleRenderer;

/**
 * Every key the loader can carry, in every spelling that can supply it — {@code
 * spec/v1/contract.schema.json}'s {@code #/$defs/schema}. Also the whole of what a producer's
 * {@code json} rendering emits.
 */
@Value
@Builder(toBuilder = true)
@Jacksonized
@JsonPropertyOrder({"schema_version", "dialect", "loader", "keys"})
public class Schema {

    /**
     * The version of this half's shape, moving independently of {@code terrace_contract}.
     * Version 2 added nesting inside {@code constraint}.
     */
    @JsonProperty("schema_version")
    int schemaVersion;

    Dialect dialect;

    @Builder.Default
    List<LoaderVar> loader = List.of();

    @Builder.Default
    List<Key> keys = List.of();

    /**
     * The schema as a JSON Schema document, for an editor or a Helm chart to validate a rendered
     * document against. Nested keys become nested {@code properties} objects.
     *
     * <p>Use {@link #toJsonSchemaWith} to choose the dialect, the title, or how strict it is.
     */
    @JsonIgnore
    public Map<String, Object> toJsonSchema() {
        return toJsonSchemaWith(JsonSchemaOptions.standard());
    }

    /** The same document, with a chosen dialect and set of annotations. See {@link JsonSchemaOptions}. */
    @JsonIgnore
    public Map<String, Object> toJsonSchemaWith(JsonSchemaOptions options) {
        return JsonSchemaRenderer.document(this, options);
    }

    /**
     * The schema as GitHub-flavoured Markdown, ready to paste into a README: the loader-variable
     * table, then the configuration keys under {@link Column#DEFAULT_COLUMNS}. Ends with a
     * newline. Use {@link #toMarkdownWith} to choose the key columns.
     */
    @JsonIgnore
    public String toMarkdown() {
        return MarkdownRenderer.toMarkdown(this);
    }

    /** Both tables, with a chosen set of key columns. See {@link Column}. */
    @JsonIgnore
    public String toMarkdownWith(List<Column> columns) {
        return MarkdownRenderer.toMarkdownWith(this, columns);
    }

    /** The loader-variable table alone. Empty when this schema has no loader variables. */
    @JsonIgnore
    public String toMarkdownLoader() {
        return MarkdownRenderer.toMarkdownLoader(this);
    }

    /** The configuration-key table alone, with a chosen set of columns. */
    @JsonIgnore
    public String toMarkdownKeys(List<Column> columns) {
        return MarkdownRenderer.toMarkdownKeys(this, columns);
    }

    /**
     * The schema as a commented {@code config.toml}, ready to be copied and edited. Use
     * {@link #toTomlExampleWith} for a shorter one.
     */
    @JsonIgnore
    public String toTomlExample() {
        return TomlExampleRenderer.toTomlExample(this);
    }

    /** The same file, with a chosen set of parts. See {@link TomlExampleOptions}. */
    @JsonIgnore
    public String toTomlExampleWith(TomlExampleOptions options) {
        return TomlExampleRenderer.toTomlExampleWith(this, options);
    }

    /**
     * Fill in each key's observed default from an already-assembled value, e.g. {@code
     * objectMapper.convertValue(defaultInstance, Map.class)}.
     *
     * <p>The Java analogue of the Rust crate's {@code Schema::with_defaults_from_value}: since
     * {@code -core} has no Jackson runtime of its own, this takes an already-nested {@code Map}
     * rather than serialising a live instance for the caller. A required key keeps no default —
     * loading fails until something supplies it — and a {@link Key#isSecret()} key's real value
     * is never carried, only the redaction {@code <redacted>}.
     */
    @JsonIgnore
    public Schema withDefaultsFromValue(Map<String, Object> root) {
        return Defaults.withDefaultsFromValue(this, root);
    }
}
