package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonIgnore;
import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import de.timscho.config.core.schema.Column;
import de.timscho.config.core.schema.Defaults;
import de.timscho.config.core.schema.JsonSchemaOptions;
import de.timscho.config.core.schema.JsonSchemaRenderer;
import de.timscho.config.core.schema.MarkdownRenderer;
import de.timscho.config.core.schema.Refine;
import de.timscho.config.core.schema.Refinement;
import de.timscho.config.core.schema.Refiner;
import de.timscho.config.core.schema.TomlExampleOptions;
import de.timscho.config.core.schema.TomlExampleRenderer;
import java.util.List;
import java.util.Map;
import lombok.Builder;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;
import org.jspecify.annotations.Nullable;

/**
 * Every key the loader can carry, in every spelling that can supply it — {@code
 * spec/v1/contract.schema.json}'s {@code #/$defs/schema}. Also the whole of what a producer's
 * {@code json} rendering emits.
 */
@Value
@Builder(toBuilder = true)
@Jacksonized
@JsonPropertyOrder({"schema_version", "dialect", "loader", "reload", "keys"})
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

    /**
     * Whether the image applies a change without restarting. Null means undeclared, and is omitted
     * rather than null-valued, so a document that says nothing about reloading is byte-identical
     * to one written before the field existed.
     */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    @Nullable ReloadSupport reload;

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
        return this.toJsonSchemaWith(JsonSchemaOptions.standard());
    }

    /** The same document, with a chosen dialect and set of annotations. See {@link JsonSchemaOptions}.
     *
     * @param options which dialect, title, and strictness to render with
     */
    @JsonIgnore
    public Map<String, Object> toJsonSchemaWith(final JsonSchemaOptions options) {
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

    /** Both tables, with a chosen set of key columns. See {@link Column}.
     *
     * @param columns which columns the configuration-key table shows, and in what order
     */
    @JsonIgnore
    public String toMarkdownWith(final List<Column> columns) {
        return MarkdownRenderer.toMarkdownWith(this, columns);
    }

    /** The loader-variable table alone. Empty when this schema has no loader variables. */
    @JsonIgnore
    public String toMarkdownLoader() {
        return MarkdownRenderer.toMarkdownLoader(this);
    }

    /** The configuration-key table alone, with a chosen set of columns.
     *
     * @param columns which columns the table shows, and in what order
     */
    @JsonIgnore
    public String toMarkdownKeys(final List<Column> columns) {
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

    /** The same file, with a chosen set of parts. See {@link TomlExampleOptions}.
     *
     * @param options which parts of the file to render
     */
    @JsonIgnore
    public String toTomlExampleWith(final TomlExampleOptions options) {
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
     *
     * @param root the already-assembled default value, nested as a {@code Map}
     */
    @JsonIgnore
    public Schema withDefaultsFromValue(final Map<String, Object> root) {
        return Defaults.withDefaultsFromValue(this, root);
    }

    /**
     * This schema with the key at {@code path} tightened beyond what its type states — the port of
     * the Rust crate's {@code Schema::refine}. See {@link Refiner} for every rule.
     *
     * <p>A default the refined constraint rejects makes the key required with no default, and
     * {@link #withDefaultsFromValue} applies the same rule, so the two may run in either order.
     *
     * @param path       the key's canonical dotted path
     * @param refinement the tightening
     * @throws de.timscho.config.core.schema.RefinementException for an unknown path, a key that is
     *                                                            not an open map, or an entry name
     *                                                            some layer could not spell
     */
    @JsonIgnore
    public Schema refine(final String path, final Refinement refinement) {
        return Refiner.refine(this, path, refinement);
    }

    /**
     * This schema with every refinement {@code source} publishes applied, its paths read relative
     * to {@code at} — the port of the Rust crate's {@code Schema::refine_with}.
     *
     * @param at     where the source's configuration is mounted, e.g. {@code legal}; empty for the root
     * @param source the library publishing the refinements
     * @throws de.timscho.config.core.schema.RefinementException as {@link #refine}
     */
    @JsonIgnore
    public Schema refineWith(final String at, final Refine source) {
        return Refiner.refineWith(this, at, source);
    }
}
