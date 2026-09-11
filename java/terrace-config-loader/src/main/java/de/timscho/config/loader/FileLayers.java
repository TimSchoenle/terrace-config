package de.timscho.config.loader;

import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeSet;

import lombok.AccessLevel;
import lombok.AllArgsConstructor;
import lombok.Getter;
import lombok.experimental.Accessors;

/**
 * The file-backed layers, collected together so the shadowing check can see all of them at once.
 */
@Accessors(fluent = true)
@AllArgsConstructor(access = AccessLevel.PRIVATE)
final class FileLayers {

    /** The secrets-directory layer, or {@code null} if none was configured — {@link Explanation}'s own use. */
    @Getter(AccessLevel.PACKAGE)
    private final SecretsDir secrets;

    private final FileSuffixEnv files;

    /**
     * Read every file-backed layer the environment points at, and apply {@code policy}.
     *
     * <p>{@code dir} is the secrets directory, when one is configured; {@code origin} names
     * whatever pointed at it.
     */
    static FileLayers collect(
            Optional<Path> dir, String origin, Dialect dialect, ShadowPolicy policy,
            Map<String, String> environment) {
        SecretsDir secrets = dir.map(d -> SecretsDir.read(origin, d, dialect)).orElse(null);
        FileSuffixEnv files = FileSuffixEnv.read(dialect, environment);
        FileLayers layers = new FileLayers(secrets, files);
        if (policy == ShadowPolicy.REJECT) {
            layers.rejectShadowedKeys(dialect, environment);
        }
        return layers;
    }

    /** Whether any file-backed value was found. */
    boolean isEmpty() {
        return (secrets == null || secrets.isEmpty()) && files.isEmpty();
    }

    /** The merged values from both file-backed layers: secrets directory, then indirection. */
    Map<String, Object> merged() {
        Map<String, Object> dict = new LinkedHashMap<>();
        if (secrets != null) {
            for (Map.Entry<String, FileValue> entry : secrets.values().entrySet()) {
                LayerValues.insertNested(dict, entry.getKey(), entry.getValue().value());
            }
        }
        for (Map.Entry<String, FileValue> entry : files.values().entrySet()) {
            LayerValues.insertNested(dict, entry.getKey(), entry.getValue().value());
        }
        return dict;
    }

    /**
     * Refuse a key supplied by more than one of: the environment, the secrets directory, the
     * indirection variables.
     */
    private void rejectShadowedKeys(Dialect dialect, Map<String, String> environment) {
        Set<String> env = dialect.plainEnvKeys(environment);
        Map<String, FileValue> secretValues = secrets == null ? Map.of() : secrets.values();
        Map<String, FileValue> fileValues = files.values();

        for (Map.Entry<String, FileValue> entry : secretValues.entrySet()) {
            String key = entry.getKey();
            if (fileValues.containsKey(key)) {
                throw shadowed(key, entry.getValue().path(), fileValues.get(key).path());
            }
            if (env.contains(key)) {
                throw shadowed(key, entry.getValue().path(), dialect.envSpelling(key));
            }
        }
        for (Map.Entry<String, FileValue> entry : fileValues.entrySet()) {
            String key = entry.getKey();
            if (env.contains(key)) {
                throw shadowed(key, entry.getValue().path(), dialect.envSpelling(key));
            }
        }
    }

    private static LoaderException shadowed(String key, Object source, Object other) {
        return new LoaderException(
                "`" + key + "` is supplied twice — by " + source + " and by " + other
                        + ". Remove one: a stale environment variable shadowing a rotated secret "
                        + "keeps the service running on the old credential.");
    }

    /** The indirection layer — {@link Explanation}'s own use. */
    FileSuffixEnv indirections() {
        return files;
    }

    /**
     * The paths a reload has to watch: the secrets directory, and every indirection target's
     * parent — directories, not files, since a Kubernetes volume update renames a whole new
     * {@code ..data} directory over the old one rather than rewriting a file in place.
     */
    Set<Path> watchPaths() {
        Set<Path> paths = new TreeSet<>();
        if (secrets != null) {
            paths.add(secrets.dir().toAbsolutePath());
        }
        paths.addAll(files.watchPaths());
        return paths;
    }

    /** Every key path collected by either file-backed layer, for tests and diagnostics. */
    Set<String> keys() {
        Set<String> keys = new TreeSet<>();
        if (secrets != null) {
            keys.addAll(secrets.values().keySet());
        }
        keys.addAll(files.values().keySet());
        return keys;
    }
}
