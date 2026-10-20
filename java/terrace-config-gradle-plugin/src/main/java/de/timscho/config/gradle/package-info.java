/**
 * The {@code de.timscho.terrace-config} Gradle plugin: a service's configuration contract, emitted
 * from its own types at build time, stamped and conformed by {@code terrace-contract}, and kept
 * equal to the copy committed beside it — see {@link de.timscho.config.gradle.TerraceContractPlugin}.
 *
 * <p>{@code @NullMarked}: every reference type in this package is non-null unless explicitly
 * marked {@link org.jspecify.annotations.Nullable @Nullable}.
 */
@NullMarked
package de.timscho.config.gradle;

import org.jspecify.annotations.NullMarked;
