package de.timscho.config.loader;

import java.io.IOException;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

import lombok.AccessLevel;
import lombok.AllArgsConstructor;
import lombok.Getter;
import lombok.experimental.Accessors;

/**
 * A directory of key-named files, which is what a Kubernetes {@code Secret} volume looks like.
 *
 * <p>Entries whose name begins with {@code .} are skipped, and so is anything that is not a
 * regular file — checked with {@link Files#isRegularFile(Path, java.nio.file.LinkOption...)}
 * with no {@code NOFOLLOW_LINKS} option, so that a projected {@code Secret} volume's per-key
 * symlinks are followed rather than classified as "not a file".
 */
@Getter(AccessLevel.PACKAGE)
@Accessors(fluent = true)
@AllArgsConstructor(access = AccessLevel.PRIVATE)
final class SecretsDir {

    /** The directory this layer was read from. */
    private final Path dir;

    /** The values this layer supplies, keyed by key path (e.g. {@code auth.jwt_secret}). */
    private final Map<String, FileValue> values;

    /**
     * Read every key-named file directly inside {@code dir}.
     *
     * <p>{@code origin} names whatever pointed at {@code dir} and is quoted back in any error.
     */
    static SecretsDir read(String origin, Path dir, Dialect dialect) {
        Map<String, FileValue> values = new LinkedHashMap<>();
        try (DirectoryStream<Path> entries = Files.newDirectoryStream(dir)) {
            for (Path entry : entries) {
                String name = entry.getFileName().toString();
                if (name.startsWith(".")) {
                    continue;
                }
                if (!Files.isRegularFile(entry)) {
                    continue;
                }
                String key = keyFromName(name, entry, dialect);
                values.put(key, new FileValue(entry, LayerValues.readValue(entry)));
            }
        } catch (IOException e) {
            throw new LoaderException(origin + " is " + dir + ", which could not be read: " + e.getMessage(), e);
        }
        return new SecretsDir(dir, values);
    }

    /** The figment key a secrets-directory file name denotes. */
    private static String keyFromName(String name, Path path, Dialect dialect) {
        if (name.contains(".")) {
            throw new LoaderException(
                    path + " is not a usable key: `.` is not the nesting separator, `"
                            + dialect.separator() + "` is (`auth" + dialect.separator()
                            + "jwt_secret` for `auth.jwt_secret`). Rename the entry, or move the "
                            + "file out of the secrets directory.");
        }

        String spelled = dialect.envSpellingOfName(name);
        if (dialect.isReserved(spelled)) {
            throw new LoaderException(
                    path + " names " + spelled + ", which is read directly from the environment "
                            + "before the layered config is built, so a file cannot supply it.");
        }
        return dialect.keyPath(name);
    }

    /** Whether the directory held no usable keys. */
    boolean isEmpty() {
        return values.isEmpty();
    }
}
