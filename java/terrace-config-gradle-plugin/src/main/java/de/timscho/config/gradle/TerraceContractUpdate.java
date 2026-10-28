package de.timscho.config.gradle;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import org.gradle.api.DefaultTask;
import org.gradle.api.file.RegularFileProperty;
import org.gradle.api.tasks.InputFile;
import org.gradle.api.tasks.OutputFile;
import org.gradle.api.tasks.PathSensitive;
import org.gradle.api.tasks.PathSensitivity;
import org.gradle.api.tasks.TaskAction;
import org.gradle.work.DisableCachingByDefault;

/**
 * Rewrites the committed contract with the unstamped emission: the {@code terraceContractUpdate}
 * task, run by hand after an intentional change to the configuration types, and reviewed under
 * {@code git diff} like any other change.
 *
 * <p>A task of its own rather than a {@code Copy} into the project directory: a {@code Copy}
 * declares the whole destination directory as its output, which would make every file beside the
 * contract one of this task's outputs.
 */
@DisableCachingByDefault(because = "Writes into the source tree, which is never restored from a cache")
public abstract class TerraceContractUpdate extends DefaultTask {

    /** Creates the task. */
    public TerraceContractUpdate() {
        // Gradle generates the subclass that implements the abstract properties.
    }

    /** {@return the unstamped document the types emit today} */
    @InputFile
    @PathSensitive(PathSensitivity.NONE)
    public abstract RegularFileProperty getEmitted();

    /** {@return the committed copy, rewritten} */
    @OutputFile
    public abstract RegularFileProperty getCommitted();

    /**
     * Copies the emission over the committed copy.
     *
     * @throws IOException if the copy fails
     */
    @TaskAction
    public void update() throws IOException {
        final File committed = this.getCommitted().get().getAsFile();
        Files.copy(
                this.getEmitted().get().getAsFile().toPath(), committed.toPath(), StandardCopyOption.REPLACE_EXISTING);
        this.getLogger().lifecycle("Wrote {}", committed);
    }
}
