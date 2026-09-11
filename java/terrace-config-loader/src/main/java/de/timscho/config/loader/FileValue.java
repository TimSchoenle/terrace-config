package de.timscho.config.loader;

import java.nio.file.Path;

import lombok.AccessLevel;
import lombok.AllArgsConstructor;
import lombok.Getter;
import lombok.experimental.Accessors;

/**
 * One value and the file it came from.
 *
 * <p>The path is kept so an error can name its source. No {@link #toString()} of this type ever
 * prints the value: the whole point of the file-backed layers is that the value stays out of
 * anything ambient, and a naive {@code toString} that dumps a credential into the first log
 * statement that touches it would give that back.
 */
@Getter
@Accessors(fluent = true)
@AllArgsConstructor(access = AccessLevel.PACKAGE)
public final class FileValue {

    /** The file this value was read from. */
    private final Path path;

    /** The value itself, minus trailing line terminators. */
    private final String value;

    @Override
    public String toString() {
        return "FileValue{path=" + path + ", value=<redacted>}";
    }
}
