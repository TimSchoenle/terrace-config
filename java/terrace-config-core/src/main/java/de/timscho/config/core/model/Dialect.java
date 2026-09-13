package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import lombok.Builder;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;

/**
 * How a key path becomes a name in each layer — {@code spec/v1/contract.schema.json}'s
 * {@code #/$defs/dialect}. All three fields are non-empty strings; an empty {@link #prefix} is
 * one of the eight build-time refusals (see
 * {@link de.timscho.config.core.refusal.EmptyPrefixException}).
 */
@Value
@Builder(toBuilder = true)
@Jacksonized
@JsonPropertyOrder({"prefix", "nesting_separator", "indirection_suffix"})
public class Dialect {

    /**
     * The environment namespace this contract governs, e.g. {@code PORTFOLIO_}. Never empty: a
     * prefixless loader cannot tell its own namespace from the machine's, and every gate in
     * {@code spec/v1/FORMAT.md} rests on that distinction.
     */
    String prefix;

    /** What separates path segments in an environment spelling, e.g. {@code __}. */
    @JsonProperty("nesting_separator")
    String nestingSeparator;

    /** What marks a variable as naming a file rather than holding a value, e.g. {@code _FILE}. */
    @JsonProperty("indirection_suffix")
    String indirectionSuffix;
}
