package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * What the loader does with a {@link LoaderVar}, from {@code spec/v1/contract.schema.json}'s
 * {@code loader_var.role}.
 *
 * <p>{@link #OTHER} is the degradation target for a role a reader does not know: a reader meeting
 * it should say it skipped the variable rather than treat it as checked.
 */
public enum LoaderRole {

    @JsonProperty("config")
    CONFIG,

    @JsonProperty("secrets_dir")
    SECRETS_DIR,

    @JsonProperty("reserved")
    RESERVED,

    @JsonProperty("other")
    OTHER
}
