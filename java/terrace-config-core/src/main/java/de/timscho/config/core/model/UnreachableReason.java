package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * Why no environment variable is published for a key. From
 * {@code spec/v1/contract.schema.json}'s {@code unreachable}.
 *
 * <p>{@link #UNNAMEABLE}: no variable names it, so the document is the only layer left.
 * {@link #INDIRECTION}: a name this schema gives another key also fills this one, which a
 * producer refuses to publish (see {@link de.timscho.config.core.refusal.IndirectionCollisionException}).
 */
public enum UnreachableReason {

    @JsonProperty("unnameable")
    UNNAMEABLE,

    @JsonProperty("indirection")
    INDIRECTION,

    @JsonProperty("other")
    OTHER
}
