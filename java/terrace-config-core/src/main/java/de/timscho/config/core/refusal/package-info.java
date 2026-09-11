/**
 * {@code spec/v1/FORMAT.md}'s "What a producer MUST refuse" — {@link
 * de.timscho.config.core.refusal.ContractValidator} and the eight {@link
 * de.timscho.config.core.refusal.ContractRefusalException} subclasses it throws.
 *
 * <p>{@code @NullMarked}: every reference type in this package is non-null unless explicitly
 * marked {@code @Nullable}. In practice nothing here needs {@code @Nullable} — every refusal
 * takes the strings it names the violation with as required arguments.
 */
@NullMarked
package de.timscho.config.core.refusal;

import org.jspecify.annotations.NullMarked;
