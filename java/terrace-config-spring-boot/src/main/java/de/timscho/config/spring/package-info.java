/**
 * {@code terrace-config-spring}: the starter and producer over Spring's {@code Binder}, target
 * tier 1 (PR 7) — see {@link de.timscho.config.spring.SpringContractProducer}, {@link
 * de.timscho.config.spring.TerraceContractAutoConfiguration} and {@link
 * de.timscho.config.spring.FileIndirectionEnvironmentPostProcessor}.
 *
 * <p>{@code @NullMarked}: every reference type in this package is non-null unless explicitly
 * marked {@link org.jspecify.annotations.Nullable @Nullable}. {@link
 * de.timscho.config.spring.TerraceContractProperties}'s fields are the one genuinely optional
 * surface: a service may leave any of them unset, which is exactly what {@code
 * @ConditionalOnProperty} and this module's own null checks account for.
 */
@NullMarked
package de.timscho.config.spring;

import org.jspecify.annotations.NullMarked;
