package de.timscho.config.loader;

import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import lombok.AccessLevel;
import lombok.AllArgsConstructor;
import org.jspecify.annotations.Nullable;

/**
 * The filesystem inputs a config was assembled from, and a fingerprint of the result — the Java
 * equivalent of the Rust crate's {@code loaded::Sources}, built by {@link TerraceLoader#loadWatched}.
 *
 * <p>The fingerprint is the fully merged nested map rather than the typed config: a target type
 * that holds a secret typically holds it in a type with no useful {@code equals} of its own, so
 * the typed value cannot be compared. Comparing the merged map instead means a reload that
 * changes nothing — a {@code ConfigMap} rewritten with identical contents, a {@code ..data} swap
 * that moved no key — is detected as a no-op before anything is torn down and rebuilt.
 *
 * <p>That fingerprint contains <b>every configuration value, secrets included</b>, which is why
 * {@link #toString()} is written by hand and redacts it. Printing a {@link Sources} must never
 * be a way to print a credential.
 */
@AllArgsConstructor(access = AccessLevel.PACKAGE)
public final class Sources {

    private final List<Path> watch;
    private final Map<String, Object> fingerprint;

    /**
     * Directories to watch for changes.
     *
     * <p>Directories, not files: a Kubernetes volume update renames a whole new {@code ..data}
     * directory over the old one, so a watch registered against a file's inode never fires a
     * second time.
     */
    public List<Path> watchPaths() {
        return this.watch;
    }

    /**
     * Whether {@code this} resolves to different values than {@code previous}.
     *
     * <p>Compared structurally rather than with {@code equals}, for one reason: a double
     * {@code NaN} is never equal to itself under IEEE 754, so a configuration holding one (a TOML
     * {@code timeout = nan}) would make a fingerprint unequal to itself and every filesystem
     * event look like a change. Floats therefore compare by their raw bits, which makes the
     * relation reflexive — the cost is that {@code 0.0} and {@code -0.0} compare unequal, which
     * is the safe direction: a needless reload of a value nobody actually writes, against a
     * reload loop that never ends.
     *
     * @param previous the previously loaded {@link Sources} to compare against
     */
    public boolean differsFrom(final Sources previous) {
        return !sameValue(this.fingerprint, previous.fingerprint);
    }

    private static boolean sameValue(@Nullable final Object first, @Nullable final Object second) {
        if (first instanceof Double left && second instanceof Double right) {
            return Double.doubleToLongBits(left) == Double.doubleToLongBits(right);
        }
        if (first instanceof Float left && second instanceof Float right) {
            return Float.floatToIntBits(left) == Float.floatToIntBits(right);
        }
        if (first instanceof Map<?, ?> left && second instanceof Map<?, ?> right) {
            return sameMap(left, right);
        }
        if (first instanceof List<?> left && second instanceof List<?> right) {
            return sameList(left, right);
        }
        return java.util.Objects.equals(first, second);
    }

    private static boolean sameMap(final Map<?, ?> left, final Map<?, ?> right) {
        if (left.size() != right.size()) {
            return false;
        }
        for (final Map.Entry<?, ?> entry : left.entrySet()) {
            if (!right.containsKey(entry.getKey())) {
                return false;
            }
            if (!sameValue(entry.getValue(), right.get(entry.getKey()))) {
                return false;
            }
        }
        return true;
    }

    private static boolean sameList(final List<?> left, final List<?> right) {
        if (left.size() != right.size()) {
            return false;
        }
        for (int i = 0; i < left.size(); i++) {
            if (!sameValue(left.get(i), right.get(i))) {
                return false;
            }
        }
        return true;
    }

    /** The watch paths, never the fingerprint. See the type's own documentation. */
    @Override
    public String toString() {
        return "Sources{watch=" + this.watch + ", fingerprint=<redacted>}";
    }
}
