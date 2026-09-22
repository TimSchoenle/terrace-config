package de.timscho.config.core.schema;

import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.UnreachableReason;
import java.util.Locale;
import java.util.regex.Pattern;
import lombok.Value;
import lombok.experimental.UtilityClass;
import org.jspecify.annotations.Nullable;

/**
 * How a key path becomes a name in each layer, and whether it can become one at all — the port of
 * the Rust crate's {@code env_spelling}, {@code env_layer_key} and {@code secrets_file_name}.
 *
 * <p>One place for the derivation, with two callers: {@link
 * de.timscho.config.core.descriptor.SchemaAssembler}, spelling every key it assembles, and {@link
 * Refiner}, refusing an entry name some layer reaching a map could not spell. Two copies of a
 * derivation are two answers to one question, and {@code FORMAT.md}'s tier 2 is exactly the claim
 * that there is only one.
 */
@UtilityClass
public class Spellings {

    /**
     * The environment spelling of {@code path}, when the environment can actually name it.
     *
     * @param dialect the prefix, separator and suffix to spell with
     * @param path    the dotted key path
     */
    public static Spelling envSpelling(final Dialect dialect, final String path) {
        final String name = rawEnvSpelling(dialect, path);
        if (!isSettableEnvName(name)) {
            return new Spelling(null, UnreachableReason.UNNAMEABLE);
        }
        if (indirectionTarget(dialect, name) != null) {
            return new Spelling(null, UnreachableReason.INDIRECTION);
        }
        final String mapped = envLayerKey(dialect, name);
        if (path.equals(mapped)) {
            return new Spelling(name, null);
        }
        return new Spelling(null, UnreachableReason.UNNAMEABLE);
    }

    /**
     * {@code path} upper-cased under the prefix, with the separator for every {@code .} — the name
     * whether or not it comes back as the same key.
     *
     * @param dialect the prefix and separator to spell with
     * @param path    the dotted key path
     */
    public static String rawEnvSpelling(final Dialect dialect, final String path) {
        return dialect.getPrefix() + path.toUpperCase(Locale.ROOT).replace(".", dialect.getNestingSeparator());
    }

    /**
     * The key a case-folding, separator-splitting environment reader makes of {@code name}, or
     * {@code null} when it drops the name.
     *
     * @param dialect the prefix and separator the reader applies
     * @param name    a full environment variable name
     */
    public static @Nullable String envLayerKey(final Dialect dialect, final String name) {
        final String trimmed = name.trim();
        if (!trimmed.startsWith(dialect.getPrefix())) {
            return null;
        }
        final String suffix = trimmed.substring(dialect.getPrefix().length());
        final String mapped = suffix.replace(dialect.getNestingSeparator(), ".").trim();
        for (final String segment : mapped.split("\\.", -1)) {
            if (segment.isEmpty()) {
                return null;
            }
        }
        return mapped.toLowerCase(Locale.ROOT);
    }

    /**
     * The key an indirection variable names, if {@code name} is one, or {@code null}.
     *
     * @param dialect the prefix and indirection suffix
     * @param name    a full environment variable name
     */
    public static @Nullable String indirectionTarget(final Dialect dialect, final String name) {
        if (!name.startsWith(dialect.getPrefix())) {
            return null;
        }
        final String rest = name.substring(dialect.getPrefix().length());
        if (!rest.endsWith(dialect.getIndirectionSuffix())) {
            return null;
        }
        final String key =
                rest.substring(0, rest.length() - dialect.getIndirectionSuffix().length());
        return key.isEmpty() ? null : key;
    }

    /**
     * The indirection variable for {@code env}, when an environment could hold it.
     *
     * @param dialect the indirection suffix
     * @param env     the variable naming the key directly
     */
    public static @Nullable String indirectionName(final Dialect dialect, final String env) {
        final String candidate = env + dialect.getIndirectionSuffix();
        return isSettableEnvName(candidate) ? candidate : null;
    }

    /**
     * The secrets-directory file name for {@code path}, when one can name it.
     *
     * @param dialect the separator a file name nests with
     * @param path    the dotted key path
     */
    public static @Nullable String secretsFileName(final Dialect dialect, final String path) {
        final String name = path.replace(".", dialect.getNestingSeparator());
        if (name.contains(".") || !isNameableFile(name)) {
            return null;
        }
        final String[] parts = name.toLowerCase(Locale.ROOT)
                .split(Pattern.quote(dialect.getNestingSeparator().toLowerCase(Locale.ROOT)), -1);
        return String.join(".", parts).equals(path) ? name : null;
    }

    private static boolean isSettableEnvName(final String name) {
        return !name.isEmpty() && name.indexOf('\0') < 0 && name.indexOf('=') < 0;
    }

    private static boolean isNameableFile(final String name) {
        return !name.isEmpty() && name.indexOf('\0') < 0 && name.indexOf('/') < 0 && name.indexOf('\\') < 0;
    }

    /** An environment spelling, or why there is none. Exactly one of the two is set. */
    @Value
    public static class Spelling {
        /** The variable naming the key, or {@code null} when none does. */
        @Nullable String env;

        /** Why no variable names the key, or {@code null} when one does. */
        @Nullable UnreachableReason unreachable;
    }
}
