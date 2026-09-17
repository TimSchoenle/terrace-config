package de.timscho.config.tck.fixtures;

import com.fasterxml.jackson.annotation.JsonProperty;
import de.timscho.config.annotations.TerraceConfig;

/**
 * How much the {@link FullSurfaceConfig} fixture says, from quietest to loudest. Also
 * {@code @TerraceConfig}, so {@link FullSurfaceConfig#logLevel}'s bare {@code @Values} can read
 * this enum's own accepted spellings — see {@code de.timscho.config.annotations.Values}'s class
 * doc for why a bare {@code @Values} needs the field's type to carry this annotation itself.
 */
@TerraceConfig
public enum LogLevel {
    @JsonProperty("trace")
    TRACE,

    @JsonProperty("debug")
    DEBUG,

    @JsonProperty("info")
    INFO,

    @JsonProperty("warn")
    WARN
}
