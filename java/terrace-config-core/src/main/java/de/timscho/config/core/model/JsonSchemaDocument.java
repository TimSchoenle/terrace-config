package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonValue;
import java.util.LinkedHashMap;
import java.util.Map;
import org.jspecify.annotations.Nullable;

/**
 * The same keys as a JSON Schema, for validating the document a chart renders: {@code
 * spec/v1/contract.schema.json}'s {@code #/$defs/json_schema}. Deliberately open: only {@code
 * $schema} is required, and the rest is whatever the producer's renderer emitted, in the order it
 * emitted it.
 *
 * <p>Held as an ordered map rather than a fixed set of fields, because this half's whole shape is
 * "the same keys as a JSON Schema", and a JSON Schema document has no closed vocabulary. Insertion
 * order is preserved, which is what keeps a re-serialised document byte-stable.
 */
public final class JsonSchemaDocument {

    private final Map<String, Object> fields;

    private JsonSchemaDocument(final Map<String, Object> fields) {
        this.fields = fields;
    }

    /**
     * Wraps a parsed JSON Schema document, keeping its key order.
     *
     * @param fields the document's top-level keys, in document order
     * @return the wrapped document, holding its own copy of {@code fields}
     * @throws IllegalArgumentException if {@code $schema} is missing, empty, or not a string
     */
    @JsonCreator
    public static JsonSchemaDocument of(final Map<String, Object> fields) {
        final Object schema = fields.get("$schema");
        if (!(schema instanceof String schemaText) || schemaText.isEmpty()) {
            throw new IllegalArgumentException("json_schema.$schema must be a non-empty string");
        }
        return new JsonSchemaDocument(new LinkedHashMap<>(fields));
    }

    /** {@return the document's top-level keys, in document order} */
    @JsonValue
    public Map<String, Object> asMap() {
        return this.fields;
    }

    /** {@return the document's {@code $schema} URI} */
    public String schemaDialect() {
        return (String) this.fields.get("$schema");
    }

    @Override
    public boolean equals(@Nullable final Object other) {
        if (this == other) {
            return true;
        }
        if (!(other instanceof JsonSchemaDocument that)) {
            return false;
        }
        return this.fields.equals(that.fields);
    }

    @Override
    public int hashCode() {
        return this.fields.hashCode();
    }

    @Override
    public String toString() {
        return "JsonSchemaDocument" + this.fields;
    }
}
