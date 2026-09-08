package de.timscho.config.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Omit the key without affecting how the field is otherwise deserialised. The escape hatch for a
 * field with genuinely no publishable shape — not a way to silence the "named type has to say
 * something" diagnostic while keeping the key. See {@code rust/docs/SCHEMA.md}, "A named type has
 * to say something".
 *
 * <p>Mirrors {@code #[config(skip)]}.
 */
@Retention(RetentionPolicy.SOURCE)
@Target(ElementType.FIELD)
public @interface Skip {
}
