package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import lombok.Builder;
import lombok.NonNull;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;

/**
 * The whole document a producer publishes — {@code spec/v1/contract.schema.json}'s root object.
 *
 * <p>This is the model {@code -loader} and {@code -spring-boot} build and {@code -spec-tck}
 * compares against the corpus; it knows nothing about either binder (see {@code
 * java/README.md}'s "Separation of concerns").
 */
@Value
@Builder(toBuilder = true)
@Jacksonized
@JsonPropertyOrder({"terrace_contract", "producer", "app", "schema", "json_schema", "external"})
public class Contract {

    /** The current envelope version. Every {@link Contract} built by this module carries this value. */
    public static final int ENVELOPE_VERSION = 1;

    /**
     * The version of this envelope's shape. A consumer refuses a version it was not written
     * against rather than reading it optimistically.
     */
    @JsonProperty("terrace_contract")
    @Builder.Default
    int terraceContract = ENVELOPE_VERSION;

    @NonNull
    Producer producer;

    @NonNull
    App app;

    @NonNull
    Schema schema;

    @NonNull
    @JsonProperty("json_schema")
    JsonSchemaDocument jsonSchema;

    @NonNull
    External external;
}
