/**
 * The Jackson-2-based {@code json} codec for {@link de.timscho.config.core.model.Contract} — see
 * java/README.md's "Dual Jackson 2 / 3 support".
 *
 * <p>{@code @NullMarked}: every reference type in this package is non-null unless explicitly
 * marked {@code @Nullable}. Nothing here needs {@code @Nullable}: every public method either
 * requires its argument or throws rather than returning one it cannot produce.
 */
@NullMarked
package de.timscho.config.core.io;

import org.jspecify.annotations.NullMarked;
