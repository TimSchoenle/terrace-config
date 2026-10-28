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
    /** Names the configuration document the loader reads. */
    @JsonProperty("config")
    CONFIG,

    /** Names the directory the loader reads secret files from. */
    @JsonProperty("secrets_dir")
    SECRETS_DIR,

    /** Read straight from the environment before the layers exist, so no file can supply it. */
    @JsonProperty("reserved")
    RESERVED,

    /** Any role this version of the format does not name. */
    @JsonProperty("other")
    OTHER
}
