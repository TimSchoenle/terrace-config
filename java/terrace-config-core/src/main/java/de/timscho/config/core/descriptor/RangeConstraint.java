package de.timscho.config.core.descriptor;

/**
 * A bound on the number a key or element accepts, as read off {@code @Range}. Each end is {@code
 * null} when unset — never all four at once, since {@code @Range} without any set is refused by
 * the processor rather than generated.
 */
public record RangeConstraint(Double min, Double max, Double exclusiveMin, Double exclusiveMax) {
}
