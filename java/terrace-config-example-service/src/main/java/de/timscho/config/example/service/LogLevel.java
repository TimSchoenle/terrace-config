package de.timscho.config.example.service;

import com.fasterxml.jackson.annotation.JsonProperty;

import de.timscho.config.annotations.TerraceConfig;

/**
 * How much the service says, from quietest to loudest.
 *
 * <p>Spelled in lower case in every configuration layer ({@code trace}, {@code debug}, ...),
 * matching {@code rust/examples/service/config.rs}'s {@code #[serde(rename_all = "lowercase")]}
 * — the constants below stay upper case regardless, because that is what a Java enum constant is.
 *
 * <p>Also {@code @TerraceConfig}, so {@code terrace-config-processor} can describe it as {@link
 * Config#getLogLevel}'s {@code @Values} — each constant's own {@code @JsonProperty} is what makes
 * the generated descriptor report {@code trace}/{@code debug}/{@code info}/{@code warn} rather
 * than the constants' own upper-case spellings.
 */
@TerraceConfig
public enum LogLevel {
    /** Every request and response body. */
    @JsonProperty("trace")
    TRACE,

    /** Every request, without bodies. */
    @JsonProperty("debug")
    DEBUG,

    /** Startup, shutdown, and anything an operator needs to see. */
    @JsonProperty("info")
    INFO,

    /** Only what is already wrong. */
    @JsonProperty("warn")
    WARN
}
