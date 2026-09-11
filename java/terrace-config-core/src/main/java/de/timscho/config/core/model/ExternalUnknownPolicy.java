package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * What to do with a variable no rule in {@code external} accounted for. From
 * {@code spec/v1/contract.schema.json}'s {@code external.unknown}; step 7 of the ordered read in
 * {@code spec/v1/FORMAT.md}.
 */
public enum ExternalUnknownPolicy {
    @JsonProperty("allow")
    ALLOW,

    @JsonProperty("warn")
    WARN,

    @JsonProperty("reject")
    REJECT
}
