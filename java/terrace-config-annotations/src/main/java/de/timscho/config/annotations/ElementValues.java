package de.timscho.config.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * The {@link Values} question, asked one level down: the field is a container, and its *element*
 * type is the one whose accepted spellings are reported, not the container itself.
 *
 * <p>Mirrors {@code #[config(element_values)]} / {@code element_values_from} / {@code
 * element_values(...)}.
 */
@Retention(RetentionPolicy.SOURCE)
@Target(ElementType.FIELD)
public @interface ElementValues {

    Class<?> from() default Void.class;

    String[] value() default {};
}
