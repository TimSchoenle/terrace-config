/**
 * The annotation vocabulary a service's own configuration types carry — {@link
 * de.timscho.config.annotations.TerraceConfig} and friends.
 *
 * <p>{@code @NullMarked}: every reference type used here is non-null unless explicitly marked
 * {@code @Nullable}. In practice nothing in this package ever needs {@code @Nullable} — every
 * type here is a {@code SOURCE}-retention {@code @interface}, and an annotation element cannot
 * return {@code null} (only a JLS-legal default, e.g. this package's own {@code Void.class} /
 * {@code Double.NaN} sentinels); {@code @NullMarked} is declared anyway so the package carries an
 * explicit nullness contract like every other package in this codebase, and so a future ordinary
 * (non-{@code @interface}) type added here inherits non-null-by-default rather than silently
 * opting out.
 */
@NullMarked
package de.timscho.config.annotations;

import org.jspecify.annotations.NullMarked;
