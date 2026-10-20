package de.timscho.config.gradle;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ExecutableLocatorTest {

    private static final String LINUX = "Linux";

    @TempDir
    Path repository;

    @Test
    void anExplicitBinaryWinsEvenWhenItIsMissing() throws IOException {
        final Path onPath = executable("bin/terrace-contract");

        final File found = ExecutableLocator.locate(
                "/nowhere/terrace-contract", rootDir(), onPath.getParent().toString(), LINUX);

        assertThat(found).isEqualTo(new File("/nowhere/terrace-contract"));
    }

    @Test
    void aRelativeExplicitBinaryIsResolvedAgainstTheRootProject() {
        final File found = ExecutableLocator.locate("tools/terrace-contract", rootDir(), null, LINUX);

        assertThat(found).isEqualTo(new File(rootDir(), "tools/terrace-contract").getAbsoluteFile());
    }

    @Test
    void pathIsSearchedInOrder() throws IOException {
        final Path first = executable("first/terrace-contract");
        final Path second = executable("second/terrace-contract");
        final String path = this.repository.resolve("empty")
                + File.pathSeparator
                + first.getParent()
                + File.pathSeparator
                + second.getParent();

        assertThat(ExecutableLocator.locate("", rootDir(), path, LINUX)).isEqualTo(first.toFile());
    }

    /**
     * A {@code cargo build} beside the build is not a place anybody named, so it is never run: a
     * consumer's build must not pick up a binary from outside its own root by accident.
     */
    @Test
    void aCargoBuildBesideTheRootProjectIsNotLookedAt() throws IOException {
        executable("cli/target/release/terrace-contract");
        executable("cli/target/debug/terrace-contract");

        assertThat(ExecutableLocator.locate(null, rootDir(), null, LINUX)).isNull();
        assertThat(ExecutableLocator.searched(null, LINUX))
                .isEqualTo("TERRACE_CONTRACT_BIN (unset) and terrace-contract in each PATH directory (PATH is unset)");
    }

    @Test
    void aFileThatIsNotExecutableIsPassedOver() throws IOException {
        final Path plain = this.repository.resolve("bin/terrace-contract");
        Files.createDirectories(plain.getParent());
        Files.writeString(plain, "");
        plain.toFile().setExecutable(false);

        assertThat(ExecutableLocator.locate(null, rootDir(), plain.getParent().toString(), LINUX))
                .isNull();
    }

    @Test
    void windowsLooksForTheExeAndTheMessageSaysSo() throws IOException {
        final Path exe = executable("bin/terrace-contract.exe");

        assertThat(ExecutableLocator.locate(null, rootDir(), exe.getParent().toString(), "Windows 11"))
                .isEqualTo(exe.toFile());
        assertThat(ExecutableLocator.searched("C:\\bin", "Windows 11"))
                .isEqualTo("TERRACE_CONTRACT_BIN (unset) and terrace-contract.exe in each PATH directory");
    }

    /** The root project's directory, one below the repository, as {@code java/} is here. */
    private File rootDir() {
        return this.repository.resolve("java").toFile();
    }

    private Path executable(final String relative) throws IOException {
        final Path file = this.repository.resolve(relative);
        Files.createDirectories(file.getParent());
        Files.writeString(file, "");
        file.toFile().setExecutable(true);
        return file;
    }
}
