/**
 * The vanilla five-layer loader (PR 6): type defaults, a TOML file/directory, prefixed
 * environment variables, a directory of key-named files, and {@code _FILE} indirection — see
 * {@link de.timscho.config.loader.TerraceLoader}, the module's single entry point.
 *
 * <p>{@code @NullMarked}: every reference type in this package is non-null unless explicitly
 * marked {@link org.jspecify.annotations.Nullable @Nullable}. A handful of fields genuinely have
 * no value until something supplies one — an unconfigured override, an absent secrets directory,
 * a key nothing shadowed — and those, and only those, carry {@code @Nullable}.
 */
@NullMarked
package de.timscho.config.loader;

import org.jspecify.annotations.NullMarked;
