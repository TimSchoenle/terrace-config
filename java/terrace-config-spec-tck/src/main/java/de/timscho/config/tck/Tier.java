package de.timscho.config.tck;

/**
 * The three conformance tiers from {@code spec/v1/CONFORMANCE.md}, in the order they nest: each
 * later tier implies every check the ones before it make.
 */
public enum Tier {

    /**
     * Schema-valid, a {@code producer} block present, and the document byte-stable across two
     * renders of the same input. The eight refusals ({@code FORMAT.md}, <em>What a producer MUST
     * refuse</em>) are not checked here — they are a property of the implementation raising them,
     * which needs {@code terrace-config-core}'s exception types (PR 4) to test directly, not a
     * property two documents can be diffed for.
     */
    TIER_1,

    /**
     * Tier 1, plus field-for-field equality of {@code env}, {@code env_file},
     * {@code secrets_file}, {@code env_aliases}, {@code env_file_aliases},
     * {@code secrets_file_aliases} and {@code unreachable} for every key.
     */
    TIER_2,

    /** Byte equality, after substituting {@code producer.version} on both sides. */
    TIER_3
}
