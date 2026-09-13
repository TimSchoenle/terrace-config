package de.timscho.config.core.descriptor;

import org.jspecify.annotations.Nullable;

/**
 * A bound on the number a key or element accepts, as read off {@code @Range}. Each end is {@code
 * null} when unset — never all four at once, since {@code @Range} without any set is refused by
 * the processor rather than generated.
 */
public record RangeConstraint(
        @Nullable Double min,
        @Nullable Double max,
        @Nullable Double exclusiveMin,
        @Nullable Double exclusiveMax) {}
