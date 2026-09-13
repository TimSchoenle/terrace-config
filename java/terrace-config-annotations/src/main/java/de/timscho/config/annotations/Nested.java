package de.timscho.config.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Recurse into the field's type instead of treating it as a leaf. The field's type — or, for a
 * container field, the type {@link Element} names — must itself carry {@link TerraceConfig}.
 *
 * <p>Mirrors {@code #[config(nested)]}. Required explicitly even when the type carries {@link
 * TerraceConfig}: a bare named type recursing on its own would make every field of every {@link
 * TerraceConfig} type reachable from any other by accident.
 */
@Retention(RetentionPolicy.SOURCE)
@Target(ElementType.FIELD)
public @interface Nested {}
