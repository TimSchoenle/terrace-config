package de.timscho.config.loader;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import lombok.AccessLevel;
import lombok.AllArgsConstructor;
import lombok.Getter;
import lombok.experimental.Accessors;

/**
 * Per-key file indirection: {@code MYAPP_<KEY>_FILE=/path}, which is what Docker Compose
 * {@code secrets:} and a number of official images look like.
 */
@Accessors(fluent = true)
@AllArgsConstructor(access = AccessLevel.PRIVATE)
final class FileSuffixEnv {

    /** The values this layer supplies, keyed by key path (e.g. {@code auth.jwt_secret}). */
    @Getter(AccessLevel.PACKAGE)
    private final Map<String, FileValue> values;

    private final Map<String, String> origins;

    /**
     * Read every indirection variable {@code dialect} recognises out of {@code environment}.
     * Scanned over the whole environment rather than looked up by name, because the keys are
     * open-ended: there is no list of them to consult.
     */
    static FileSuffixEnv read(final Dialect dialect, final Map<String, String> environment) {
        final String prefix = dialect.prefix();
        final Map<String, FileValue> values = new LinkedHashMap<>();
        final Map<String, String> origins = new LinkedHashMap<>();

        for (final Map.Entry<String, String> entry : environment.entrySet()) {
            final String name = entry.getKey();
            final Optional<String> target = dialect.indirectionTarget(name);
            if (target.isEmpty()) {
                continue;
            }
            final String key = target.get();

            final String spelled = prefix + key;
            if (dialect.isReserved(spelled)) {
                throw new LoaderException(name + " is set, but " + spelled + " is read directly from the "
                        + "environment before the layered config is built, so a file "
                        + "cannot supply it. Set " + spelled + " itself.");
            }

            final Path path = Path.of(entry.getValue());
            final String value;
            try {
                value = LayerValues.readValue(path);
            } catch (LoaderException e) {
                throw new LoaderException(name + " names " + path + ": " + e.getMessage(), e);
            }
            final String keyPath = dialect.keyPath(key);
            origins.put(keyPath, name);
            values.put(keyPath, new FileValue(path, value));
        }

        return new FileSuffixEnv(values, origins);
    }

    /** The variable that named the file supplying {@code key}, in the spelling it was set in. */
    Optional<String> origin(final String key) {
        return Optional.ofNullable(this.origins.get(key));
    }

    /** Whether the environment declared no indirection variables. */
    boolean isEmpty() {
        return this.values.isEmpty();
    }

    /**
     * The directories a reload has to watch: the parent of every named path.
     *
     * <p>Parent directories rather than the files themselves — a Kubernetes volume update
     * replaces the file by renaming a new {@code ..data} directory over the old one, so a watch
     * registered against the old inode never fires again.
     */
    List<Path> watchPaths() {
        final List<Path> paths = new ArrayList<>();
        for (final FileValue value : this.values.values()) {
            final Path parent = value.path().toAbsolutePath().getParent();
            if (parent != null) {
                paths.add(parent);
            }
        }
        return paths;
    }
}
