package de.timscho.config.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Renders the observed default as {@code <redacted>} and marks the key as {@code secret}. A
 * producer refuses to publish a secret key that also carries a default (see {@code
 * spec/v1/FORMAT.md}, "What a producer MUST refuse").
 *
 * <p>Mirrors {@code #[config(secret)]}.
 */
@Retention(RetentionPolicy.SOURCE)
@Target(ElementType.FIELD)
public @interface Secret {}
