package de.timscho.config.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Report the spellings a key accepts. Exactly one of the three forms is used per field:
 *
 * <ul>
 *   <li>bare {@code @Values} — the field's own type carries {@link TerraceConfig} as an enum, and
 *       its constants are the accepted spellings;
 *   <li>{@code @Values(from = Mirror.class)} — another type, itself an enum carrying {@link
 *       TerraceConfig}, reports the spellings this field's actual type accepts (for a foreign
 *       enum a {@code @TerraceConfig} mirror can be written for);
 *   <li>{@code @Values({"a", "b"})} — a literal list, for a foreign enum no mirror can be written
 *       for.
 * </ul>
 *
 * <p>Mirrors {@code #[config(values)]} / {@code values_from} / {@code values(...)}. See {@code
 * rust/docs/SCHEMA.md}, "Values a trait cannot reach", for why the three are not interchangeable:
 * only the accepted set the field's actual deserialiser produces is honest to publish.
 */
@Retention(RetentionPolicy.SOURCE)
@Target(ElementType.FIELD)
public @interface Values {

    Class<?> from() default Void.class;

    String[] value() default {};
}
