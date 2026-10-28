package de.timscho.config.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Bounds the number the key accepts. At least one of the four is set. Each bound is published as
 * written: this producer does not intersect it with the range of the field's own type, so {@code max}
 * above {@code Short.MAX_VALUE} on a {@code short} field publishes that {@code maximum}, and the
 * loader still refuses a value the type cannot hold. The Rust derive drops such a bound instead
 * ({@code rust/docs/SCHEMA.md}, "A number the type is too wide for").
 *
 * <p>On a scalar field it bounds the value. On a container field it bounds the element, since a
 * bound on the container itself would mean nothing, and it takes the place of {@link Element} and
 * {@link ElementValues}: a container field carries at most one of the three.
 *
 * <p>{@link Double#NaN} marks an end as unset, since no finite double can otherwise mean that.
 *
 * <p>Mirrors {@code #[config(range(...))]}.
 */
@Retention(RetentionPolicy.SOURCE)
@Target(ElementType.FIELD)
public @interface Range {

    /** {@return the inclusive lower bound, or {@link Double#NaN} when the key has none} */
    double min() default Double.NaN;

    /** {@return the inclusive upper bound, or {@link Double#NaN} when the key has none} */
    double max() default Double.NaN;

    /** {@return the exclusive lower bound, or {@link Double#NaN} when the key has none} */
    double exclusiveMin() default Double.NaN;

    /** {@return the exclusive upper bound, or {@link Double#NaN} when the key has none} */
    double exclusiveMax() default Double.NaN;
}
