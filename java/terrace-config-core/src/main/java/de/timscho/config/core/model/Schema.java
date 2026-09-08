package de.timscho.config.core.model;

import java.util.List;

import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import lombok.Builder;
import lombok.NonNull;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;

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

    @NonNull
    Dialect dialect;

    @NonNull
    @Builder.Default
    List<LoaderVar> loader = List.of();

    @NonNull
    @Builder.Default
    List<Key> keys = List.of();
}
