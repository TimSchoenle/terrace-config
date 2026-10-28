package de.timscho.config.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Bounds the number the key accepts. At least one of the four is set; a bound that would *widen*
 * one the type itself already justifies (e.g. {@code max} above {@code Short.MAX_VALUE} on a {@code
 * short} field) is dropped rather than published, as {@code rust/docs/SCHEMA.md} explains under "A
 * number the type is too wide for".
 *
 * <p>Applies to the field itself, or, combined with {@link Element}, to a container's element; the
 * two never combine on one field, since a bound on the container itself would mean nothing.
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
