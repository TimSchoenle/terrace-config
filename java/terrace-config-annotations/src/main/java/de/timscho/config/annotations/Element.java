package de.timscho.config.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * The field is a container — {@code Optional}, {@code List}, {@code Set} or {@code Map} — and its
 * *element* type carries {@link TerraceConfig} as a struct, with keys of its own. Reports a nested
 * schema on the key, not new top-level keys.
 *
 * <p>Mirrors {@code #[config(element)]}. Found through {@code Optional}, into the item of a {@code
 * List}/{@code Set}, and into the *value* of a {@code Map} — a map's key type is skipped, since a
 * TOML table's keys are strings whatever the map is keyed by.
 */
@Retention(RetentionPolicy.SOURCE)
@Target(ElementType.FIELD)
public @interface Element {
}
