package de.timscho.config.core.model;

import java.util.LinkedHashMap;
import java.util.Map;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonValue;
import org.jspecify.annotations.Nullable;

/**
 * The same keys as a JSON Schema, for validating the document a chart renders — {@code
 * spec/v1/contract.schema.json}'s {@code #/$defs/json_schema}. Deliberately open: only {@code
 * $schema} is required, and the rest is whatever the producer's renderer emitted, in the order
 * it emitted it.
 *
 * <p>Held as an ordered map rather than a fixed set of fields, because this half's whole shape is
 * "the same keys as a JSON Schema" — a JSON Schema document has no closed vocabulary. Insertion
 * order is preserved, which is what keeps a re-serialised document byte-stable.
 */
public final class JsonSchemaDocument {

    private final Map<String, Object> fields;

    private JsonSchemaDocument(Map<String, Object> fields) {
        this.fields = fields;
    }

    @JsonCreator
    public static JsonSchemaDocument of(Map<String, Object> fields) {
        Object schema = fields.get("$schema");
        if (!(schema instanceof String schemaText) || schemaText.isEmpty()) {
            throw new IllegalArgumentException("json_schema.$schema must be a non-empty string");
        }
        return new JsonSchemaDocument(new LinkedHashMap<>(fields));
    }

    @JsonValue
    public Map<String, Object> asMap() {
        return fields;
    }

    public String schemaDialect() {
        return (String) fields.get("$schema");
    }

    @Override
    public boolean equals(@Nullable Object other) {
        if (this == other) {
            return true;
        }
        if (!(other instanceof JsonSchemaDocument that)) {
            return false;
        }
        return fields.equals(that.fields);
    }

    @Override
    public int hashCode() {
        return fields.hashCode();
    }

    @Override
    public String toString() {
        return "JsonSchemaDocument" + fields;
    }
}
