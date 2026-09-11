/**
 * The JSR-269 annotation processor generating {@code <Type>Descriptor} classes from
 * {@code @TerraceConfig}-annotated types (PR 5) — see {@link
 * de.timscho.config.processor.TerraceConfigProcessor}.
 *
 * <p>{@code @NullMarked}: every reference type in this package is non-null unless explicitly
 * marked {@link org.jspecify.annotations.Nullable @Nullable}. Exception: the four fields {@link
 * de.timscho.config.processor.TerraceConfigProcessor} assigns in its overridden {@code init}
 * rather than in a constructor are declared non-null by convention, not because they cannot be
 * {@code null} before {@code init} runs — {@code javax.annotation.processing.Processor}'s own
 * lifecycle guarantees {@code init} completes before {@code process} is ever called, which is the
 * same "initializer method, not constructor" exemption tools like NullAway grant framework
 * lifecycle methods.
 */
@NullMarked
package de.timscho.config.processor;

import org.jspecify.annotations.NullMarked;
