package de.timscho.config.loader;

import java.util.Collections;
import java.util.LinkedHashSet;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;

import lombok.AccessLevel;
import lombok.AllArgsConstructor;
import lombok.Getter;
import lombok.With;
import lombok.experimental.Accessors;

/**
 * How a deployment spells its configuration keys in its environment.
 *
 * <p>Every name {@link TerraceLoader} reads or reports is derived from one {@code Dialect}: the
 * prefix, the nesting separator, the {@code _FILE} suffix, and the handful of keys that are read
 * before the layers exist. Immutable; each {@code with}-style method returns a new instance,
 * mirroring the Rust crate's own consuming builder.
 *
 * <pre>{@code
 * Dialect dialect = Dialect.of("MYAPP_");
 * assert dialect.keyPath("AUTH__JWT_SECRET").equals("auth.jwt_secret");
 * assert dialect.envSpelling("auth.jwt_secret").equals("MYAPP_AUTH__JWT_SECRET");
 * }</pre>
 */
@Getter
@Accessors(fluent = true)
@AllArgsConstructor(access = AccessLevel.PRIVATE)
public final class Dialect {

    private static final String DEFAULT_SEPARATOR = "__";
    private static final String DEFAULT_FILE_SUFFIX = "_FILE";

    /** The prefix every configuration variable carries. */
    private final String prefix;

    /** What separates nesting levels in an environment key. */
    private final String separator;

    /** What marks a variable holding a path rather than a value. */
    @With
    private final String fileSuffix;

    private final Set<String> reserved;

    /** A dialect over {@code prefix}, with {@code __} nesting and the {@code _FILE} suffix. */
    public static Dialect of(String prefix) {
        return new Dialect(prefix, DEFAULT_SEPARATOR, DEFAULT_FILE_SUFFIX, Collections.emptySet());
    }

    /** Replace the nesting separator. Defaults to {@code __}. */
    public Dialect withNestingSeparator(String separator) {
        return new Dialect(prefix, separator, fileSuffix, reserved);
    }

    /**
     * Reserve a key, in its <b>full environment spelling</b>, e.g. {@code MYAPP_PROFILE}.
     *
     * <p>A reserved key is read straight from the environment, before or outside the layered
     * config, so a file cannot supply it. Matching is case-insensitive, see {@link
     * #isReserved(String)}.
     */
    public Dialect reserve(String key) {
        Set<String> next = new LinkedHashSet<>(reserved);
        next.add(key.toUpperCase(Locale.ROOT));
        return new Dialect(prefix, separator, fileSuffix, Collections.unmodifiableSet(next));
    }

    /** What marks a variable holding a path rather than a value. */
    public String indirectionSuffix() {
        return fileSuffix;
    }

    /** Whether {@code name} — a full environment spelling — is reserved. Case-insensitive. */
    public boolean isReserved(String name) {
        return reserved.contains(name.toUpperCase(Locale.ROOT));
    }

    /**
     * An environment key suffix ({@code AUTH__JWT_SECRET}) as a key path ({@code
     * auth.jwt_secret}). The separator is matched case-insensitively.
     */
    public String keyPath(String suffix) {
        String lowered = suffix.toLowerCase(Locale.ROOT);
        String[] parts = lowered.split(java.util.regex.Pattern.quote(separator.toLowerCase(Locale.ROOT)), -1);
        return String.join(".", parts);
    }

    /** A key path back in its environment spelling, for error messages. */
    public String envSpelling(String key) {
        return prefix + key.toUpperCase(Locale.ROOT).replace(".", separator);
    }

    /** The environment spelling of a bare key name, without treating {@code .} as nesting. */
    String envSpellingOfName(String name) {
        return prefix + name.toUpperCase(Locale.ROOT);
    }

    /**
     * The key an indirection variable names, if {@code name} is one — {@code
     * <PREFIX><KEY><SUFFIX>} with a non-empty {@code <KEY>}.
     */
    java.util.Optional<String> indirectionTarget(String name) {
        if (!name.startsWith(prefix)) {
            return java.util.Optional.empty();
        }
        String rest = name.substring(prefix.length());
        if (!rest.endsWith(fileSuffix)) {
            return java.util.Optional.empty();
        }
        String key = rest.substring(0, rest.length() - fileSuffix.length());
        return key.isEmpty() ? java.util.Optional.empty() : java.util.Optional.of(key);
    }

    /**
     * Every key the environment supplies directly, against the variables that supply it.
     * Excludes {@code _FILE} indirections and reserved keys.
     */
    Map<String, Set<String>> plainEnvEntries(Map<String, String> environment) {
        Map<String, Set<String>> keys = new TreeMap<>();
        for (String name : environment.keySet()) {
            if (isReserved(name)) {
                continue;
            }
            if (indirectionTarget(name).isPresent()) {
                continue;
            }
            if (name.startsWith(prefix)) {
                String suffix = name.substring(prefix.length());
                if (!suffix.isEmpty()) {
                    keys.computeIfAbsent(keyPath(suffix), k -> new TreeSet<>()).add(name);
                }
            }
        }
        return keys;
    }

    /** Every key the environment supplies directly. */
    Set<String> plainEnvKeys(Map<String, String> environment) {
        return new TreeSet<>(plainEnvEntries(environment).keySet());
    }
}
