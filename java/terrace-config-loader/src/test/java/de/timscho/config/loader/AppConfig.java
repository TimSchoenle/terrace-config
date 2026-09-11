package de.timscho.config.loader;

import com.fasterxml.jackson.annotation.JsonProperty;

import de.timscho.config.annotations.Nested;
import de.timscho.config.annotations.Range;
import de.timscho.config.annotations.Secret;
import de.timscho.config.annotations.TerraceConfig;
import de.timscho.config.annotations.Values;

/**
 * An annotated fixture for {@link TerraceLoaderSchemaTest}, processed at compile time by {@code
 * terrace-config-processor} into a real {@code AppConfigDescriptor}, so the schema-assembly
 * tests exercise generated descriptors rather than hand-built ones.
 */
@TerraceConfig
class AppConfig {

    @Nested
    Database database;

    @Values
    LogLevel level;

    @Secret
    String token;

    @Range(min = 0, max = 65535)
    int retries;

    // Deliberately camelCase: the environment layer folds a variable name to lower case on the
    // way in, so this path can never come back from the environment the way `retries` can. See
    // `retryLimit` below for the annotated fix to exactly this problem.
    int retryCount;

    // `@JsonProperty` is what `TerraceLoader.load`'s own Jackson binding already keys off for a
    // renamed field, and the processor reads the same annotation (`FieldResolver.resolve`) so the
    // generated descriptor's key path agrees with what the loader actually reads — unlike
    // `retryCount` above, `TEST_MAX_RETRIES` genuinely reaches this field.
    @JsonProperty("max_retries")
    int retryLimit;

    @TerraceConfig
    static class Database {
        /** The connection string. */
        String url;

        int port;
    }

    @TerraceConfig
    enum LogLevel {
        TRACE,
        DEBUG,
        INFO
    }
}
