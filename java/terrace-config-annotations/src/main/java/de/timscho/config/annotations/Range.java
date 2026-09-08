package de.timscho.config.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Bound the number the key accepts. At least one of the four is set; a bound that would *widen*
 * one the type itself already justifies (e.g. {@code max} above {@code Short.MAX_VALUE} on a
 * {@code short} field) is dropped rather than published — see {@code rust/docs/SCHEMA.md}, "A
 * number the type is too wide for".
 *
 * <p>Applies to the field itself, or — combined with {@link Element} — to a container's element;
 * the two never combine on one field, since a bound on the container itself would mean nothing.
 *
 * <p>{@link Double#NaN} marks an end as unset, since no finite double can otherwise mean that.
 *
 * <p>Mirrors {@code #[config(range(...))]}.
 */
@Retention(RetentionPolicy.SOURCE)
@Target(ElementType.FIELD)
public @interface Range {

    double min() default Double.NaN;

    double max() default Double.NaN;

    double exclusiveMin() default Double.NaN;

    double exclusiveMax() default Double.NaN;
}
