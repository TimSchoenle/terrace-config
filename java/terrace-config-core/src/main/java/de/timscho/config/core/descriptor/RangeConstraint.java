package de.timscho.config.core.descriptor;

import org.jspecify.annotations.Nullable;

/**
 * A bound on the number a key or element accepts, as read off {@code @Range}. Each end is {@code
 * null} when unset, though never all four at once, since {@code @Range} without any set is refused
 * by the processor rather than generated.
 *
 * @param min the inclusive lower bound, or {@code null}
 * @param max the inclusive upper bound, or {@code null}
 * @param exclusiveMin the exclusive lower bound, or {@code null}
 * @param exclusiveMax the exclusive upper bound, or {@code null}
 */
public record RangeConstraint(
        @Nullable Double min,
        @Nullable Double max,
        @Nullable Double exclusiveMin,
        @Nullable Double exclusiveMax) {}
