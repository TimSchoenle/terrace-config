package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * Whether a rebuild applies a change to one key, from {@code spec/v1/contract.schema.json}'s
 * {@code key.reload}.
 *
 * <p>Absent — a {@code null} {@link Key#getReload()} — means undeclared, which a consumer reads as
 * {@link #RESTART}. See {@code spec/v1/FORMAT.md}'s "Reloading".
 */
public enum Reload {
    /** The value is consumed inside the runtime a rebuild reconstructs, so a rebuild applies it. */
    @JsonProperty("live")
    LIVE,

    /** The value is applied only at process start; a rebuild keeps it at its boot value. */
    @JsonProperty("restart")
    RESTART
}
