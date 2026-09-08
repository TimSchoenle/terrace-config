package de.timscho.config.loader;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;

/**
 * Per-key file indirection: {@code MYAPP_<KEY>_FILE=/path}, which is what Docker Compose
 * {@code secrets:} and a number of official images look like.
 */
final class FileSuffixEnv {

    private final Map<String, FileValue> values;
    private final Map<String, String> origins;

    private FileSuffixEnv(Map<String, FileValue> values, Map<String, String> origins) {
        this.values = values;
        this.origins = origins;
    }

    /**
     * Read every indirection variable {@code dialect} recognises out of {@code environment}.
     * Scanned over the whole environment rather than looked up by name, because the keys are
     * open-ended: there is no list of them to consult.
     */
    static FileSuffixEnv read(Dialect dialect, Map<String, String> environment) {
        String prefix = dialect.prefix();
        Map<String, FileValue> values = new LinkedHashMap<>();
        Map<String, String> origins = new LinkedHashMap<>();

        for (Map.Entry<String, String> entry : environment.entrySet()) {
            String name = entry.getKey();
            Optional<String> target = dialect.indirectionTarget(name);
            if (target.isEmpty()) {
                continue;
            }
            String key = target.get();

            String spelled = prefix + key;
            if (dialect.isReserved(spelled)) {
                throw new LoaderException(
                        name + " is set, but " + spelled + " is read directly from the "
                                + "environment before the layered config is built, so a file "
                                + "cannot supply it. Set " + spelled + " itself.");
            }

            Path path = Path.of(entry.getValue());
            String value;
            try {
                value = LayerValues.readValue(path);
            } catch (LoaderException e) {
                throw new LoaderException(name + " names " + path + ": " + e.getMessage(), e);
            }
            String keyPath = dialect.keyPath(key);
            origins.put(keyPath, name);
            values.put(keyPath, new FileValue(path, value));
        }

        return new FileSuffixEnv(values, origins);
    }

    /** The values this layer supplies, keyed by key path (e.g. {@code auth.jwt_secret}). */
    Map<String, FileValue> values() {
        return values;
    }

    /** The variable that named the file supplying {@code key}, in the spelling it was set in. */
    Optional<String> origin(String key) {
        return Optional.ofNullable(origins.get(key));
    }

    /** Whether the environment declared no indirection variables. */
    boolean isEmpty() {
        return values.isEmpty();
    }

    /**
     * The directories a reload has to watch: the parent of every named path.
     *
     * <p>Parent directories rather than the files themselves — a Kubernetes volume update
     * replaces the file by renaming a new {@code ..data} directory over the old one, so a watch
     * registered against the old inode never fires again.
     */
    List<Path> watchPaths() {
        List<Path> paths = new ArrayList<>();
        for (FileValue value : values.values()) {
            Path parent = value.path().toAbsolutePath().getParent();
            if (parent != null) {
                paths.add(parent);
            }
        }
        return paths;
    }
}
