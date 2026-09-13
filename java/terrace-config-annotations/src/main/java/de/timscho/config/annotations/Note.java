package de.timscho.config.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Prose qualifying the observed default, e.g. {@code "permanent"} for a zero that means no expiry.
 *
 * <p>Mirrors {@code #[config(note = "…")]}.
 */
@Retention(RetentionPolicy.SOURCE)
@Target(ElementType.FIELD)
public @interface Note {

    String value();
}
