package de.timscho.config.loader;

import java.nio.file.Path;

/**
 * One value and the file it came from.
 *
 * <p>The path is kept so an error can name its source. No {@link #toString()} of this type ever
 * prints the value: the whole point of the file-backed layers is that the value stays out of
 * anything ambient, and a naive {@code toString} that dumps a credential into the first log
 * statement that touches it would give that back.
 */
public final class FileValue {

    private final Path path;
    private final String value;

    FileValue(Path path, String value) {
        this.path = path;
        this.value = value;
    }

    /** The file this value was read from. */
    public Path path() {
        return path;
    }

    /** The value itself, minus trailing line terminators. */
    public String value() {
        return value;
    }

    @Override
    public String toString() {
        return "FileValue{path=" + path + ", value=<redacted>}";
    }
}
