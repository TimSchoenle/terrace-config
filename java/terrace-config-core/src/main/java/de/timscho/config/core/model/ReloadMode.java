package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonProperty;

/** How an image applies a configuration change after it has started, from {@code schema.reload.mode}. */
public enum ReloadMode {
    /** Nothing is applied after start. */
    @JsonProperty("none")
    NONE,

    /**
     * A debounced change to a watched layer re-reads every layer and rebuilds the runtime, keeping
     * every {@link Reload#RESTART} key at its boot value.
     */
    @JsonProperty("rebuild")
    REBUILD
}
