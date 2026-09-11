package de.timscho.config.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Marks a type whose configuration keys (or, on an enum, whose constants) are read by {@code
 * terrace-config-processor}. The Java answer to {@code #[derive(Describe)]} — see {@code
 * rust/docs/SCHEMA.md}.
 *
 * <p>On a class or record, each non-static, non-transient field becomes a key. On an enum, the
 * constants become the values one key accepts, for a field elsewhere annotated {@link Values}
 * pointing at this type.
 *
 * <p>Retained at {@link RetentionPolicy#SOURCE}: nothing at runtime reads this annotation, only
 * the processor during compilation.
 */
@Retention(RetentionPolicy.SOURCE)
@Target(ElementType.TYPE)
public @interface TerraceConfig {}
