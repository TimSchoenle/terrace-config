/**
 * The document model — the envelope every contract renders from, regardless of which loader or
 * producer built it.
 *
 * <p>{@code @NullMarked}: every reference type in this package is non-null unless explicitly
 * marked {@link org.jspecify.annotations.Nullable @Nullable}. Several of these records model an
 * optional build/config field that is genuinely absent (not merely empty) when the producer never
 * supplied it — those fields, and only those, carry {@code @Nullable}.
 */
@NullMarked
package de.timscho.config.core.model;

import org.jspecify.annotations.NullMarked;
