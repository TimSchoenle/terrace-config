package de.timscho.config.loader;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import org.tomlj.Toml;
import org.tomlj.TomlArray;
import org.tomlj.TomlParseResult;
import org.tomlj.TomlTable;

/**
 * The TOML layer: one file, or every {@code *.toml} in a directory.
 */
final class TomlLayers {

    /** How deep a TOML fragment is walked by {@link #fragmentKeys} before the rest of a branch is reported as one path. */
    private static final int MAX_DEPTH = 32;

    private final Path root;
    private final List<Path> files;

    private TomlLayers(Path root, List<Path> files) {
        this.root = root;
        this.files = files;
    }

    /**
     * Expand {@code path} into the TOML files it denotes.
     *
     * <p>A plain path is returned as-is, missing or not — a missing file is simply skipped when
     * merged, which is deliberate: a service with no configuration file at all is the normal
     * development case. A <b>directory</b> is expanded to every {@code *.toml} directly inside
     * it, sorted by name so a {@code 10-base.toml} / {@code 20-overrides.toml} pair merges in the
     * order an operator reading the mount would predict.
     *
     * <p>Dot-prefixed entries are skipped, matching {@link SecretsDir}: a Kubernetes {@code
     * ConfigMap} volume is a directory of symlinks beside a {@code ..data} directory.
     */
    static TomlLayers expand(String origin, Path path) {
        if (!Files.isDirectory(path)) {
            return new TomlLayers(path, List.of(path));
        }

        List<Path> found = new ArrayList<>();
        try (DirectoryStream<Path> entries = Files.newDirectoryStream(path)) {
            for (Path entry : entries) {
                String name = entry.getFileName().toString();
                if (name.startsWith(".")) {
                    continue;
                }
                if (!Files.isRegularFile(entry)) {
                    continue;
                }
                String extension = extensionOf(name);
                if (extension.equalsIgnoreCase("toml")) {
                    found.add(entry);
                }
            }
        } catch (IOException e) {
            throw new LoaderException(origin + " is " + path + ", which could not be read: " + e.getMessage(), e);
        }
        Collections.sort(found);
        return new TomlLayers(path, found);
    }

    private static String extensionOf(String name) {
        int dot = name.lastIndexOf('.');
        return dot < 0 ? "" : name.substring(dot + 1);
    }

    /** The files, in merge order. */
    List<Path> files() {
        return files;
    }

    /**
     * The directory a reload would have to watch, if there is one — the directory itself when
     * {@code root} names one, otherwise its parent (unless it names a bare file name with none).
     */
    java.util.Optional<Path> watchDir() {
        if (Files.isDirectory(root)) {
            return java.util.Optional.of(root);
        }
        Path parent = root.toAbsolutePath().getParent();
        return java.util.Optional.ofNullable(parent);
    }

    /**
     * Every key path {@code path} supplies, or {@code null} if it will not parse as TOML —
     * {@link Explanation}'s own reporting, kept separate from {@link #merged()} since a report
     * has to name each file's own keys rather than the merged result.
     */
    static List<String> fragmentKeys(Path path) {
        TomlParseResult result;
        try {
            result = Toml.parse(path);
        } catch (IOException e) {
            return null;
        }
        if (result.hasErrors()) {
            return null;
        }
        List<String> keys = new ArrayList<>();
        pushLeaves(result, "", keys, 0);
        return keys;
    }

    /** Push the dotted path of every leaf in {@code table} into {@code keys}. */
    private static void pushLeaves(TomlTable table, String prefix, List<String> keys, int depth) {
        for (String segment : table.keySet()) {
            String path = prefix.isEmpty() ? segment : prefix + "." + segment;
            Object value = table.get(segment);
            // An empty table is a leaf: it is a path the file really does mention, and a
            // fragment holding nothing else would otherwise report as supplying nothing.
            if (value instanceof TomlTable inner && !inner.isEmpty() && depth < MAX_DEPTH) {
                pushLeaves(inner, path, keys, depth + 1);
            } else {
                keys.add(path);
            }
        }
    }

    /** Every file, parsed and deep-merged in order into one nested map. */
    Map<String, Object> merged() {
        Map<String, Object> merged = new LinkedHashMap<>();
        for (Path file : files) {
            if (!Files.isRegularFile(file)) {
                continue;
            }
            TomlParseResult result;
            try {
                result = Toml.parse(file);
            } catch (IOException e) {
                throw new UncheckedIOException("reading " + file + ": " + e.getMessage(), e);
            }
            if (result.hasErrors()) {
                StringBuilder message = new StringBuilder();
                for (Object error : result.errors()) {
                    if (message.length() > 0) {
                        message.append("; ");
                    }
                    message.append(error);
                }
                throw new LoaderException(file + " is not valid TOML: " + message);
            }
            LayerValues.deepMerge(merged, convertTable(result));
        }
        return merged;
    }

    /**
     * A {@link TomlTable} converted into a plain, deeply nested {@link Map} — {@code
     * TomlTable#toMap()} leaves nested tables as {@code TomlTable} instances rather than
     * converting them, which Jackson then misreads as a bean (picking up methods like {@code
     * isEmpty()} as a spurious {@code "empty"} property) instead of a JSON object.
     */
    private static Map<String, Object> convertTable(TomlTable table) {
        Map<String, Object> map = new LinkedHashMap<>();
        for (String key : table.keySet()) {
            map.put(key, convertValue(table.get(key)));
        }
        return map;
    }

    private static List<Object> convertArray(TomlArray array) {
        List<Object> list = new ArrayList<>();
        for (int i = 0; i < array.size(); i++) {
            list.add(convertValue(array.get(i)));
        }
        return list;
    }

    private static Object convertValue(Object value) {
        if (value instanceof TomlTable table) {
            return convertTable(table);
        }
        if (value instanceof TomlArray array) {
            return convertArray(array);
        }
        return value;
    }
}
