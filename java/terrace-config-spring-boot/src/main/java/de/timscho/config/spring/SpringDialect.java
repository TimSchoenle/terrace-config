package de.timscho.config.spring;

import java.util.Locale;
import java.util.Optional;

import lombok.AccessLevel;
import lombok.AllArgsConstructor;
import lombok.Getter;
import lombok.With;
import lombok.experimental.Accessors;

/**
 * How a name in the system environment maps onto a Spring configuration property, for the one
 * feature this module adds that Spring's own relaxed binding does not already give it: {@code
 * _FILE} indirection.
 *
 * <p><b>This is not {@code terrace-config-loader}'s {@code Dialect}, and the two disagree on
 * purpose.</b> The vanilla loader's nesting separator is {@code __} (two underscores), chosen
 * precisely so a segment name may itself contain a single underscore (Rust's own naming
 * convention) without colliding with nesting. Spring's relaxed binding already owns the
 * environment-to-property mapping before this module ever runs, and it treats <em>one</em>
 * underscore as the nesting separator (`MYAPP_GITHUB_TOKEN` binds `myapp.github.token`) — a
 * convention this module cannot change without breaking every property Spring already binds
 * correctly. The consequence: a single environment variable such as {@code MYAPP_GITHUB_TOKEN}
 * is ambiguous between "the key {@code github.token}" and "the key {@code github_token}" the
 * moment the target type has a field actually named with an underscore, in a way {@code __}
 * never is. There is no fix that does not re-implement Spring's own binder; this class exists to
 * make the divergence explicit and testable rather than silent.
 *
 * <p>Immutable; {@code with}-style methods return a new instance.
 */
@Getter
@Accessors(fluent = true)
@AllArgsConstructor(access = AccessLevel.PRIVATE)
public final class SpringDialect {

    private static final String DEFAULT_SEPARATOR = "_";
    private static final String DEFAULT_INDIRECTION_SUFFIX = "_FILE";

    /** What separates nesting levels in an environment variable name. */
    private final String separator;

    /** What marks a variable as naming a file rather than holding a value directly. */
    @With
    private final String indirectionSuffix;

    /** Spring's own relaxed-binding dialect: {@code _} nesting, {@code _FILE} indirection. */
    public static SpringDialect standard() {
        return new SpringDialect(DEFAULT_SEPARATOR, DEFAULT_INDIRECTION_SUFFIX);
    }

    /** Replace the nesting separator. Defaults to {@code _}, matching Spring's own binder. */
    public SpringDialect withNestingSeparator(String separator) {
        return new SpringDialect(separator, indirectionSuffix);
    }

    /** Whether {@code envName} is a {@code _FILE}-suffixed indirection, with a non-empty target. */
    public boolean isIndirection(String envName) {
        return indirectionTarget(envName).isPresent();
    }

    /**
     * The environment variable name {@code envName} points a file's contents at, if {@code
     * envName} is a well-formed indirection — non-empty, and not itself all suffix.
     */
    public Optional<String> indirectionTarget(String envName) {
        if (!envName.endsWith(indirectionSuffix)) {
            return Optional.empty();
        }
        String target = envName.substring(0, envName.length() - indirectionSuffix.length());
        return target.isEmpty() ? Optional.empty() : Optional.of(target);
    }

    /**
     * {@code envName} as a Spring configuration property name: folded to lower case, {@link
     * #separator()} replaced with {@code .}. Matches — deliberately no more precisely than — the
     * relaxed binding Spring's own {@code Binder} already applies to a real environment variable,
     * so a value this class publishes under this name binds exactly where the equivalent
     * environment variable would have.
     */
    public String propertyName(String envName) {
        return envName.toLowerCase(Locale.ROOT).replace(separator, ".");
    }
}
