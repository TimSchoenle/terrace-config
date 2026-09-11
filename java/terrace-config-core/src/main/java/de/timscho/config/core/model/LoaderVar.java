package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import lombok.Builder;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;
import org.jspecify.annotations.Nullable;

/**
 * A variable the loader reads to decide what the layers are, rather than to fill a key —
 * {@code spec/v1/contract.schema.json}'s {@code #/$defs/loader_var}. Step 1 of the ordered read
 * in {@code spec/v1/FORMAT.md}.
 */
@Value
@Builder
@Jacksonized
@JsonPropertyOrder({"env", "role", "docs", "default"})
public class LoaderVar {

    String env;

    LoaderRole role;

    String docs;

    /** What the loader assumes when the variable is unset. Null when there is no default. */
    @JsonProperty("default")
    @Nullable String defaultValue;
}
