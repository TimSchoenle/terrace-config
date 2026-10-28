package de.timscho.config.gradle;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;
import javax.inject.Inject;
import org.gradle.api.DefaultTask;
import org.gradle.api.GradleException;
import org.gradle.api.file.ConfigurableFileCollection;
import org.gradle.api.file.RegularFileProperty;
import org.gradle.api.provider.ListProperty;
import org.gradle.api.provider.Property;
import org.gradle.api.tasks.CacheableTask;
import org.gradle.api.tasks.Classpath;
import org.gradle.api.tasks.Input;
import org.gradle.api.tasks.Nested;
import org.gradle.api.tasks.OutputFile;
import org.gradle.api.tasks.TaskAction;
import org.gradle.jvm.toolchain.JavaLauncher;
import org.gradle.process.ExecOperations;
import org.gradle.process.ExecResult;

/**
 * Emits the service's contract, unstamped, by running the producer's build-time entry point on the
 * service's runtime classpath: no application context, no network, nothing the service does at
 * boot.
 *
 * <p>Cacheable because the document is a pure function of what is declared here (the classes, the
 * entry point and its options, and the JVM) and deliberately of nothing else: build identity
 * (version, revision, a timestamp) is {@link TerraceContractStamp}'s, so a new commit or a new
 * second never invalidates this task's output.
 */
@CacheableTask
public abstract class TerraceContractEmit extends DefaultTask {

    /** Creates the task. */
    public TerraceContractEmit() {
        // Gradle generates the subclass that implements the abstract properties.
    }

    /** {@return the service's classes and runtime dependencies, plus the generator's own} */
    @Classpath
    public abstract ConfigurableFileCollection getClasspath();

    /** {@return the entry point: {@code ContractMain} or {@code SpringContractMain}} */
    @Input
    public abstract Property<String> getMainClass();

    /** {@return the root {@code @TerraceConfig} type, by binary name} */
    @Input
    public abstract Property<String> getType();

    /** {@return the environment prefix} */
    @Input
    public abstract Property<String> getPrefix();

    /** {@return the contract's {@code app.name}} */
    @Input
    public abstract Property<String> getAppName();

    /** {@return reserved keys, each passed as {@code --reserve}} */
    @Input
    public abstract ListProperty<String> getReserve();

    /** {@return further options, passed as they are} */
    @Input
    public abstract ListProperty<String> getProducerArguments();

    /**
     * {@return the JVM the entry point runs on: the project's toolchain, so the classes load as
     * compiled}
     */
    @Nested
    public abstract Property<JavaLauncher> getJavaLauncher();

    /** {@return the unstamped document} */
    @OutputFile
    public abstract RegularFileProperty getOutput();

    /** {@return the process launcher the entry point runs through} */
    @Inject
    protected abstract ExecOperations getExecOperations();

    /**
     * Runs the entry point and fails with what it printed when it refuses or is misused.
     *
     * @throws IOException if the previous document cannot be deleted
     */
    @TaskAction
    public void emit() throws IOException {
        final File output = this.getOutput().get().getAsFile();
        // The entry point writes only on success; removing the previous document first means a
        // failed run never leaves a stale one behind for the next task to read.
        Files.deleteIfExists(output.toPath());
        final List<String> arguments = new ArrayList<>();
        arguments.add("--type");
        arguments.add(this.getType().get());
        arguments.add("--prefix");
        arguments.add(this.getPrefix().get());
        arguments.add("--app-name");
        arguments.add(this.getAppName().get());
        for (final String reserved : this.getReserve().get()) {
            arguments.add("--reserve");
            arguments.add(reserved);
        }
        arguments.addAll(this.getProducerArguments().get());
        arguments.add("--out");
        arguments.add(output.getAbsolutePath());

        final ByteArrayOutputStream printed = new ByteArrayOutputStream();
        final ExecResult result = this.getExecOperations().javaexec(spec -> {
            spec.setExecutable(this.getJavaLauncher().get().getExecutablePath().getAsFile());
            spec.setClasspath(this.getClasspath());
            spec.getMainClass().set(this.getMainClass());
            spec.setArgs(arguments);
            // Not the project directory: the entry point reads no file, and a working directory
            // nobody else writes to keeps it that way by construction.
            spec.setWorkingDir(this.getTemporaryDir());
            spec.setStandardOutput(printed);
            spec.setErrorOutput(printed);
            spec.setIgnoreExitValue(true);
        });
        if (result.getExitValue() != 0) {
            throw new GradleException(this.getMainClass().get() + " exited " + result.getExitValue()
                    + " (1 refused, 2 usage or classpath error):" + System.lineSeparator()
                    + printed.toString(StandardCharsets.UTF_8).strip());
        }
        if (!output.isFile()) {
            throw new GradleException(this.getMainClass().get() + " exited 0 but wrote no " + output);
        }
    }
}
