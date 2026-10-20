/**
 * What a {@link de.timscho.config.core.model.Schema} needs before it can be published: refinements
 * a type cannot state, observed defaults, environment spellings, and the JSON Schema half a
 * contract embeds as {@code json_schema}.
 *
 * <p>Every other rendering (Markdown, a commented {@code config.toml}, a standalone JSON Schema,
 * image labels) is a pure function of the published document and is {@code terrace-contract
 * render}'s, never this package's ({@code docs/contract-cli-plan.md} §2).
 *
 * <p>{@code @NullMarked}: every reference type in this package is non-null unless explicitly
 * marked {@link org.jspecify.annotations.Nullable @Nullable}.
 */
@NullMarked
package de.timscho.config.core.schema;

import org.jspecify.annotations.NullMarked;
