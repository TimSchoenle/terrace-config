package de.timscho.config.example.springservice;

import de.timscho.config.annotations.TerraceConfig;

/**
 * How much the service says, from quietest to loudest.
 *
 * <p>Unlike {@code terrace-config-example-service}'s own {@code LogLevel}, this one needs no
 * {@code @JsonProperty} to accept a lower-case configuration value: Spring's own {@code Binder}
 * matches an enum constant case-insensitively and folds {@code -}/{@code _} on the way in, so
 * {@code orders.log-level=debug}, {@code ORDERS_LOG_LEVEL=DEBUG} and {@code Debug} all bind here
 * without help. The generated contract reports the canonical upper-case spellings below; Spring
 * accepts more than the contract insists a validator recognise.
 */
@TerraceConfig
public enum LogLevel {
    /** Every request and response body. */
    TRACE,

    /** Every request, without bodies. */
    DEBUG,

    /** Startup, shutdown, and anything an operator needs to see. */
    INFO,

    /** Only what is already wrong. */
    WARN
}
