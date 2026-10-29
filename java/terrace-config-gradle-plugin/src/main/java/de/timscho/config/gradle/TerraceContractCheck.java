package de.timscho.config.gradle;

import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.Arrays;
import java.util.List;
import org.gradle.api.DefaultTask;
import org.gradle.api.GradleException;
import org.gradle.api.file.RegularFileProperty;
import org.gradle.api.provider.Property;
import org.gradle.api.tasks.InputFile;
import org.gradle.api.tasks.InputFiles;
import org.gradle.api.tasks.Internal;
import org.gradle.api.tasks.PathSensitive;
import org.gradle.api.tasks.PathSensitivity;
import org.gradle.api.tasks.TaskAction;
import org.gradle.work.DisableCachingByDefault;

/**
 * Fails when the committed contract is not byte-for-byte the document the service's types emit
 * today: the {@code terraceContractCheck} task, which {@code check} depends on.
 *
 * <p>Compared with the unstamped emission, not the stamped one, so the committed copy carries no
 * version, commit or timestamp and changes only when the configuration surface does, which is
 * exactly when a reviewer should see it change.
 */
@DisableCachingByDefault(because = "Comparing two small files is cheaper than a cache lookup")
public abstract class TerraceContractCheck extends DefaultTask {

    /** Creates the task. */
    public TerraceContractCheck() {
        // Gradle generates the subclass that implements the abstract properties.
    }

    /** {@return the unstamped document the types emit today} */
    @InputFile
    @PathSensitive(PathSensitivity.NONE)
    public abstract RegularFileProperty getEmitted();

    /**
     * {@return the committed copy} {@code @InputFiles} rather than {@code @InputFile}, so a missing
     * file reaches this task's own message instead of failing input validation.
     */
    @InputFiles
    @PathSensitive(PathSensitivity.NONE)
    public abstract RegularFileProperty getCommitted();

    /** {@return the task path that rewrites the committed copy, for the message} */
    @Internal
    public abstract Property<String> getUpdateTaskPath();

    /**
     * Compares the two documents.
     *
     * @throws IOException if either document cannot be read
     */
    @TaskAction
    public void compare() throws IOException {
        final File committed = this.getCommitted().get().getAsFile();
        final String fix = " Run `gradle " + this.getUpdateTaskPath().get() + "` and commit the result.";
        if (!committed.isFile()) {
            throw new GradleException(committed + " does not exist." + fix);
        }
        final byte[] expected =
                Files.readAllBytes(this.getEmitted().get().getAsFile().toPath());
        final byte[] actual = Files.readAllBytes(committed.toPath());
        if (Arrays.equals(expected, actual)) {
            return;
        }
        throw new GradleException(committed + " is not what the configuration types emit today"
                + firstDifference(actual, expected) + "." + fix);
    }

    /** Where the two first differ, as a line number and both lines, so the message is actionable. */
    static String firstDifference(final byte[] committed, final byte[] emitted) {
        final List<String> was = lines(committed);
        final List<String> is = lines(emitted);
        final int common = Math.min(was.size(), is.size());
        for (int line = 0; line < common; line++) {
            if (!was.get(line).equals(is.get(line))) {
                return describe(line + 1, was.get(line), is.get(line));
            }
        }
        if (was.size() != is.size()) {
            return describe(
                    common + 1,
                    common < was.size() ? was.get(common) : "<end of file>",
                    common < is.size() ? is.get(common) : "<end of file>");
        }
        return ", differing only in line endings or a final newline";
    }

    private static List<String> lines(final byte[] document) {
        return Arrays.asList(new String(document, StandardCharsets.UTF_8).split("\\R"));
    }

    private static String describe(final int line, final String committed, final String emitted) {
        return ": first at line " + line + System.lineSeparator() + "  committed: " + committed.strip()
                + System.lineSeparator() + "  emitted:   " + emitted.strip() + System.lineSeparator();
    }
}
