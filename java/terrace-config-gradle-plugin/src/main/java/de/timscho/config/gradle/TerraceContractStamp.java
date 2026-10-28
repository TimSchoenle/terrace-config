package de.timscho.config.gradle;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.IOException;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;
import javax.inject.Inject;
import org.gradle.api.DefaultTask;
import org.gradle.api.GradleException;
import org.gradle.api.file.RegularFileProperty;
import org.gradle.api.provider.Property;
import org.gradle.api.tasks.Input;
import org.gradle.api.tasks.InputFile;
import org.gradle.api.tasks.Internal;
import org.gradle.api.tasks.Optional;
import org.gradle.api.tasks.OutputFile;
import org.gradle.api.tasks.PathSensitive;
import org.gradle.api.tasks.PathSensitivity;
import org.gradle.api.tasks.TaskAction;
import org.gradle.process.ExecOperations;
import org.gradle.process.ExecResult;
import org.gradle.work.DisableCachingByDefault;
import org.jspecify.annotations.Nullable;

/**
 * Stamps the emitted contract with the build's identity through {@code terrace-contract stamp},
 * then holds the stamped document to {@code terrace-contract conform} at the producer's tier: the
 * {@code terraceContract} task, whose output is the document an image ships.
 *
 * <p>Up-to-date checking applies, but caching does not: both steps take milliseconds, and an
 * output carrying a timestamp would rarely be asked for twice. A document that does not conform is
 * deleted, so nothing downstream can pick up a contract its own build rejected.
 */
@DisableCachingByDefault(because = "Stamping and conforming are cheaper than a cache lookup")
public abstract class TerraceContractStamp extends DefaultTask {

    /** Creates the task. */
    public TerraceContractStamp() {
        // Gradle generates the subclass that implements the abstract properties.
    }

    /** {@return the unstamped document {@link TerraceContractEmit} wrote} */
    @InputFile
    @PathSensitive(PathSensitivity.NONE)
    public abstract RegularFileProperty getUnstamped();

    /**
     * {@return the binary; absent when none was found, which fails the task with {@link
     * #getSearched()}}
     */
    @InputFile
    @Optional
    @PathSensitive(PathSensitivity.NONE)
    public abstract RegularFileProperty getExecutable();

    /** {@return where the binary was looked for, for the message when it was not found} */
    @Internal
    public abstract Property<String> getSearched();

    /** {@return the tier {@code conform} holds the stamped document to} */
    @Input
    public abstract Property<Integer> getTier();

    /** {@return {@code app.version}, or nothing} */
    @Input
    @Optional
    public abstract Property<String> getAppVersion();

    /** {@return {@code app.revision}, or nothing} */
    @Input
    @Optional
    public abstract Property<String> getRevision();

    /** {@return {@code app.created}, or nothing} */
    @Input
    @Optional
    public abstract Property<String> getCreated();

    /** {@return {@code app.source}, or nothing} */
    @Input
    @Optional
    public abstract Property<String> getSource();

    /** {@return the stamped, conformed document} */
    @OutputFile
    public abstract RegularFileProperty getOutput();

    /** {@return the process launcher the binary runs through} */
    @Inject
    protected abstract ExecOperations getExecOperations();

    /**
     * Stamps, then conforms; deletes the output and fails when either step does.
     *
     * @throws IOException if the output cannot be deleted, created or written
     */
    @TaskAction
    public void stampAndConform() throws IOException {
        if (!this.getExecutable().isPresent()) {
            throw new GradleException("terrace-contract was not found. Looked at "
                    + this.getSearched().get()
                    + ". Install it on PATH (in GitHub Actions, the terrace-contract install action in"
                    + " TimSchoenle/actions), or name it with"
                    + " `terraceContract { executable = file(...) }` or " + ExecutableLocator.BIN_VARIABLE + ".");
        }
        final String executable = this.getExecutable().get().getAsFile().getAbsolutePath();
        final File output = this.getOutput().get().getAsFile();
        Files.deleteIfExists(output.toPath());
        Files.createDirectories(output.toPath().toAbsolutePath().getParent());

        final List<String> stamp = new ArrayList<>(List.of(executable, "stamp"));
        addOption(stamp, "--app-version", this.getAppVersion().getOrNull());
        addOption(stamp, "--revision", this.getRevision().getOrNull());
        addOption(stamp, "--created", this.getCreated().getOrNull());
        addOption(stamp, "--source", this.getSource().getOrNull());
        stamp.add(this.getUnstamped().get().getAsFile().getAbsolutePath());
        final ByteArrayOutputStream stampErrors = new ByteArrayOutputStream();
        try (OutputStream document = Files.newOutputStream(output.toPath())) {
            final int status = this.run(stamp, document, stampErrors);
            if (status != 0) {
                throw this.failure(output, stamp, status, stampErrors);
            }
        }
        this.warn(stampErrors);

        final List<String> conform = List.of(
                executable, "conform", "--tier", String.valueOf(this.getTier().get()), output.getAbsolutePath());
        final ByteArrayOutputStream findings = new ByteArrayOutputStream();
        final int status = this.run(conform, findings, findings);
        if (status != 0) {
            throw this.failure(output, conform, status, findings);
        }
        this.warn(findings);
    }

    /** What a successful run still printed (a warning about terrace.yaml, say), so it is not lost. */
    private void warn(final ByteArrayOutputStream printed) {
        final String text = printed.toString(StandardCharsets.UTF_8).strip();
        if (!text.isEmpty()) {
            this.getLogger().warn(text);
        }
    }

    private int run(final List<String> command, final OutputStream out, final OutputStream err) {
        final ExecResult result = this.getExecOperations().exec(spec -> {
            spec.commandLine(command);
            spec.setStandardOutput(out);
            spec.setErrorOutput(err);
            spec.setIgnoreExitValue(true);
        });
        return result.getExitValue();
    }

    private GradleException failure(
            final File output, final List<String> command, final int status, final ByteArrayOutputStream printed)
            throws IOException {
        Files.deleteIfExists(output.toPath());
        return new GradleException(String.join(" ", command) + " exited " + status
                + " (1 findings, 2 the document could not be read):" + System.lineSeparator()
                + printed.toString(StandardCharsets.UTF_8).strip());
    }

    private static void addOption(final List<String> command, final String option, final @Nullable String value) {
        if (value != null && !value.isBlank()) {
            command.add(option);
            command.add(value);
        }
    }
}
