package de.timscho.config.gradle;

import java.io.File;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import org.jspecify.annotations.Nullable;

/**
 * Finds the {@code terrace-contract} binary when the extension does not name one.
 *
 * <p>In order: {@code TERRACE_CONTRACT_BIN}, then the first {@code terrace-contract} on {@code
 * PATH}, and nothing else. Every place is one the build's owner named, explicitly or by putting it
 * on {@code PATH}: the plugin never runs a binary from a directory nobody pointed it at, and never
 * downloads one, since the binary that holds a build's contract to the specification is one the
 * build's owner chose and can audit.
 */
final class ExecutableLocator {

    /** The variable naming the binary explicitly. */
    static final String BIN_VARIABLE = "TERRACE_CONTRACT_BIN";

    private static final String BASE_NAME = "terrace-contract";

    private ExecutableLocator() {}

    /**
     * The binary, or {@code null} when neither place has one.
     *
     * <p>A {@code TERRACE_CONTRACT_BIN} that is set is returned as it is, existing or not: it is an
     * explicit instruction, and the task's own input validation then names the path that is
     * missing, rather than this quietly looking somewhere else. A relative one is resolved against
     * {@code rootDir}, not against the Gradle daemon's working directory, which the build does not
     * choose.
     *
     * @param explicit the value of {@code TERRACE_CONTRACT_BIN}, or {@code null} when unset
     * @param rootDir  the root project's directory, which a relative {@code explicit} is resolved
     *                 against
     * @param path     the value of {@code PATH}, or {@code null} when unset
     * @param osName   the {@code os.name} system property, deciding the {@code .exe} suffix
     */
    static @Nullable File locate(
            final @Nullable String explicit, final File rootDir, final @Nullable String path, final String osName) {
        if (explicit != null && !explicit.isBlank()) {
            final File named = new File(explicit);
            return named.isAbsolute() ? named : new File(rootDir, explicit).getAbsoluteFile();
        }
        for (final File candidate : onPath(path, osName)) {
            if (candidate.isFile() && candidate.canExecute()) {
                return candidate;
            }
        }
        return null;
    }

    /** Every place {@link #locate} looks, in its order, for a message saying why it found nothing. */
    static String searched(final @Nullable String path, final String osName) {
        final StringBuilder message = new StringBuilder(BIN_VARIABLE)
                .append(" (unset) and ")
                .append(fileName(osName))
                .append(" in each PATH directory");
        if (path == null || path.isBlank()) {
            message.append(" (PATH is unset)");
        }
        return message.toString();
    }

    private static List<File> onPath(final @Nullable String path, final String osName) {
        final String name = fileName(osName);
        final List<File> candidates = new ArrayList<>();
        if (path != null) {
            for (final String directory : path.split(File.pathSeparator)) {
                if (!directory.isEmpty()) {
                    candidates.add(new File(directory, name));
                }
            }
        }
        return candidates;
    }

    private static String fileName(final String osName) {
        return osName.toLowerCase(Locale.ROOT).startsWith("windows") ? BASE_NAME + ".exe" : BASE_NAME;
    }
}
