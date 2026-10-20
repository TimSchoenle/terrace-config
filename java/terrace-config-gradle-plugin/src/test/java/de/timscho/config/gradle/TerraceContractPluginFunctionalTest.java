package de.timscho.config.gradle;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import org.gradle.testkit.runner.BuildResult;
import org.gradle.testkit.runner.GradleRunner;
import org.gradle.testkit.runner.TaskOutcome;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/**
 * Runs the plugin in real builds through Gradle TestKit.
 *
 * <p>The build under test carries stub entry points in place of {@code ContractMain} and {@code
 * SpringContractMain}: each copies a fixture document to {@code --out} and records the options it
 * was given, so these tests exercise the plugin's own wiring (classpath, options, caching, stamp,
 * conform, check) without compiling the producers into every build. The producers' own documents
 * are held to {@code conform} by the TCK and by the examples' {@code terraceContract}.
 *
 * <p>Tests that stamp or conform need the real binary; see {@link TerraceContractBinary} for what
 * happens without it.
 */
class TerraceContractPluginFunctionalTest {

    /** A corpus document every tier accepts, as the stub entry point's output. */
    private static final Path FIXTURE = Path.of(System.getProperty("terrace.plugin.fixture"));

    /** A corpus document that meets tier 1 and not tier 2. */
    private static final Path TIER_ONE_ONLY = TerraceContractBinary.repositoryRoot()
            .toPath()
            .resolve("spec/v1/conformance/tier-1-spellings/contract.json");

    @TempDir
    Path project;

    @BeforeEach
    void writeSettings() throws IOException {
        write("settings.gradle.kts", """
                rootProject.name = "orders"
                buildCache { local { directory = file(".build-cache") } }
                """);
        write("gradle.properties", "org.gradle.caching=true\n");
    }

    @Test
    void stampsTheEmissionWithTheBuildsIdentityAndConformsIt() throws IOException {
        final File binary = TerraceContractBinary.require();
        loaderBuild(FIXTURE, "");

        final Map<String, String> environment = environment(binary);
        environment.put("TERRACE_CONTRACT_CREATED", "2026-01-02T03:04:05Z");
        final BuildResult result = runner(
                        environment, "terraceContract", "-PterraceContract.revision=abc123", "--configuration-cache")
                .build();

        assertThat(result.task(":terraceContract").getOutcome()).isEqualTo(TaskOutcome.SUCCESS);
        final String stamped = read("build/terrace/contract.json");
        assertThat(stamped)
                .contains("\"version\": \"1.2.3\"")
                .contains("\"revision\": \"abc123\"")
                .contains("\"created\": \"2026-01-02T03:04:05Z\"")
                .contains("\"source\": \"https://example.com/orders\"");
        assertThat(read("build/args.txt"))
                .isEqualTo("--type com.example.Config --prefix ORDERS_ --app-name orders"
                        + " --reserve ORDERS_PROFILE --fixture " + FIXTURE + " --record "
                        + this.project.resolve("build/args.txt"));
    }

    @Test
    void theEmissionIsCacheableAndNoStampedValueInvalidatesIt() throws IOException {
        loaderBuild(FIXTURE, "");

        final BuildResult first = runner(
                        environment(null), "terraceContractEmit", "-PterraceContract.created=2026-01-01T00:00:00Z")
                .build();
        final BuildResult restamped = runner(
                        environment(null), "terraceContractEmit", "-PterraceContract.created=2027-01-01T00:00:00Z")
                .build();
        runner(environment(null), "clean").build();
        final BuildResult fromCache =
                runner(environment(null), "terraceContractEmit").build();

        assertThat(first.task(":terraceContractEmit").getOutcome()).isEqualTo(TaskOutcome.SUCCESS);
        assertThat(restamped.task(":terraceContractEmit").getOutcome()).isEqualTo(TaskOutcome.UP_TO_DATE);
        assertThat(fromCache.task(":terraceContractEmit").getOutcome()).isEqualTo(TaskOutcome.FROM_CACHE);
    }

    @Test
    void checkPassesWhenTheCommittedContractIsTheEmission() throws IOException {
        loaderBuild(FIXTURE, "");
        Files.copy(FIXTURE, this.project.resolve("contract.json"));

        final BuildResult result = runner(environment(null), "check").build();

        assertThat(result.task(":terraceContractCheck").getOutcome()).isEqualTo(TaskOutcome.SUCCESS);
    }

    @Test
    void checkNamesTheFirstDifferenceAndTheTaskThatFixesIt() throws IOException {
        loaderBuild(FIXTURE, "");
        final String committed = Files.readString(FIXTURE).replace("\"minimal\"", "\"renamed\"");
        write("contract.json", committed);

        final BuildResult result = runner(environment(null), "check").buildAndFail();

        assertThat(result.getOutput())
                .contains("is not what the configuration types emit today: first at line")
                .contains("\"renamed\"")
                .contains("Run `gradle :terraceContractUpdate` and commit the result.");
    }

    @Test
    void checkFailsWhenNoContractIsCommitted() throws IOException {
        loaderBuild(FIXTURE, "");

        final BuildResult result =
                runner(environment(null), "terraceContractCheck").buildAndFail();

        assertThat(result.getOutput()).contains("contract.json does not exist.");
    }

    @Test
    void updateRewritesTheCommittedContract() throws IOException {
        loaderBuild(FIXTURE, "");
        write("contract.json", "{}\n");

        runner(environment(null), "terraceContractUpdate").build();
        final BuildResult check =
                runner(environment(null), "terraceContractCheck").build();

        assertThat(this.project.resolve("contract.json")).hasSameBinaryContentAs(FIXTURE);
        assertThat(check.task(":terraceContractCheck").getOutcome()).isEqualTo(TaskOutcome.SUCCESS);
    }

    @Test
    void aContractBelowTheProducersTierFailsAndIsNotLeftBehind() throws IOException {
        final File binary = TerraceContractBinary.require();
        loaderBuild(TIER_ONE_ONLY, "");

        final BuildResult result =
                runner(environment(binary), "terraceContract").buildAndFail();

        assertThat(result.getOutput()).contains("conform --tier 2").contains("exited 1");
        assertThat(this.project.resolve("build/terrace/contract.json")).doesNotExist();
    }

    @Test
    void aMissingBinarySaysWhereItWasLookedFor() throws IOException {
        loaderBuild(FIXTURE, "");
        final Map<String, String> environment = environment(null);
        environment.remove(ExecutableLocator.BIN_VARIABLE);
        environment.put("PATH", this.project.resolve("no-binary-here").toString());

        final BuildResult result = runner(environment, "terraceContract").buildAndFail();

        assertThat(result.getOutput())
                .contains("terrace-contract was not found. Looked at TERRACE_CONTRACT_BIN (unset) and"
                        + " terrace-contract in each PATH directory")
                .contains("terraceContract { executable = file(...) }");
    }

    @Test
    void theProducerAndItsTierAreInferredFromTheRuntimeClasspath() throws IOException {
        stubModule("terrace-config-spring-boot", "de.timscho.config.spring", "SpringContractMain");
        write("build.gradle.kts", """
                plugins { id("de.timscho.terrace-config") }
                dependencies { implementation(project(":terrace-config-spring-boot")) }
                terraceContract {
                    type = "com.example.Config"
                    prefix = "ORDERS_"
                    producerArguments.addAll("--fixture", "%s", "--record", "%s")
                }
                tasks.register("printTier") {
                    val tier = terraceContract.tier
                    doLast { println("tier=" + tier.get()) }
                }
                """.formatted(FIXTURE, this.project.resolve("build/args.txt")));
        appendSetting("include(\"terrace-config-spring-boot\")");

        final BuildResult result =
                runner(environment(null), "terraceContractEmit", "printTier").build();

        assertThat(result.getOutput()).contains("tier=1");
        assertThat(read("build/args.txt")).startsWith("de.timscho.config.spring.SpringContractMain ");
    }

    @Test
    void bothProducersOnTheClasspathAskForTheLoaderToBeSet() throws IOException {
        stubModule("terrace-config-spring-boot", "de.timscho.config.spring", "SpringContractMain");
        stubModule("terrace-config-loader", "de.timscho.config.loader", "ContractMain");
        write("build.gradle.kts", """
                plugins { id("de.timscho.terrace-config") }
                dependencies {
                    implementation(project(":terrace-config-spring-boot"))
                    implementation(project(":terrace-config-loader"))
                }
                terraceContract {
                    type = "com.example.Config"
                    prefix = "ORDERS_"
                }
                """);
        appendSetting("include(\"terrace-config-spring-boot\", \"terrace-config-loader\")");

        final BuildResult result =
                runner(environment(null), "terraceContractEmit").buildAndFail();

        assertThat(result.getOutput())
                .contains("Both terrace-config-loader and terrace-config-spring-boot are on the runtime classpath")
                .contains("loader = \"terrace-java\"");
    }

    @Test
    void aRefusingEntryPointFailsTheEmissionWithWhatItPrinted() throws IOException {
        loaderBuild(FIXTURE, "producerArguments.addAll(\"--refuse\", \"key `a` is unnameable\")");

        final BuildResult result =
                runner(environment(null), "terraceContractEmit").buildAndFail();

        assertThat(result.getOutput())
                .contains("de.timscho.config.loader.ContractMain exited 1")
                .contains("error: refusing to emit this contract: key `a` is unnameable");
    }

    /** A single-project build whose stub {@code ContractMain} emits {@code fixture}. */
    private void loaderBuild(final Path fixture, final String extra) throws IOException {
        stubEntryPoint(this.project, "de.timscho.config.loader", "ContractMain");
        write("build.gradle.kts", """
                plugins { id("de.timscho.terrace-config") }
                version = "1.2.3"
                terraceContract {
                    type = "com.example.Config"
                    prefix = "ORDERS_"
                    loader = "terrace-java"
                    reserve.add("ORDERS_PROFILE")
                    source = "https://example.com/orders"
                    producerArguments.addAll("--fixture", "%s", "--record", "%s")
                    %s
                }
                """.formatted(fixture, this.project.resolve("build/args.txt"), extra));
    }

    /** A subproject named like a producer's module, carrying that producer's stub entry point. */
    private void stubModule(final String name, final String packageName, final String className) throws IOException {
        final Path module = this.project.resolve(name);
        Files.createDirectories(module);
        Files.writeString(module.resolve("build.gradle.kts"), "plugins { java }\n");
        stubEntryPoint(module, packageName, className);
    }

    /**
     * An entry point that copies {@code --fixture} to {@code --out}, writes the options before
     * {@code --out} (preceded by its own name when it is not {@code ContractMain}) to {@code
     * --record}, and with {@code --refuse <reason>} refuses the way the real ones do.
     */
    private static void stubEntryPoint(final Path module, final String packageName, final String className)
            throws IOException {
        final Path source = module.resolve("src/main/java")
                .resolve(packageName.replace('.', '/'))
                .resolve(className + ".java");
        Files.createDirectories(source.getParent());
        final String self = "ContractMain".equals(className) ? "\"\"" : "\"" + packageName + "." + className + " \"";
        Files.writeString(source, """
                package %s;

                import java.nio.file.Files;
                import java.nio.file.Path;
                import java.util.ArrayList;
                import java.util.List;

                public final class %s {
                    public static void main(String[] args) throws Exception {
                        String fixture = null;
                        String record = null;
                        String out = null;
                        List<String> given = new ArrayList<>();
                        for (int i = 0; i < args.length; i += 2) {
                            switch (args[i]) {
                                case "--fixture" -> fixture = args[i + 1];
                                case "--record" -> record = args[i + 1];
                                case "--out" -> out = args[i + 1];
                                case "--refuse" -> {
                                    System.err.println("error: refusing to emit this contract: " + args[i + 1]);
                                    System.exit(1);
                                }
                                default -> { }
                            }
                            if (!"--out".equals(args[i])) {
                                given.add(args[i]);
                                given.add(args[i + 1]);
                            }
                        }
                        if (record != null) {
                            Files.createDirectories(Path.of(record).getParent());
                            Files.writeString(Path.of(record), %s + String.join(" ", given));
                        }
                        Files.copy(Path.of(fixture), Path.of(out));
                    }
                }
                """.formatted(packageName, className, self));
    }

    private GradleRunner runner(final Map<String, String> environment, final String... arguments) {
        final List<String> all = new ArrayList<>(List.of(arguments));
        all.add("--stacktrace");
        return GradleRunner.create()
                .withProjectDir(this.project.toFile())
                .withPluginClasspath()
                .withEnvironment(environment)
                .withArguments(all)
                .forwardOutput();
    }

    /** This process's environment, with {@code TERRACE_CONTRACT_BIN} naming {@code binary} or unset. */
    private static Map<String, String> environment(final File binary) {
        final Map<String, String> environment = new HashMap<>(System.getenv());
        environment.remove(ExecutableLocator.BIN_VARIABLE);
        environment.remove("TERRACE_CONTRACT_REVISION");
        environment.remove("TERRACE_CONTRACT_CREATED");
        if (binary != null) {
            environment.put(ExecutableLocator.BIN_VARIABLE, binary.getAbsolutePath());
        }
        return environment;
    }

    private void appendSetting(final String line) throws IOException {
        Files.writeString(
                this.project.resolve("settings.gradle.kts"),
                line + System.lineSeparator(),
                java.nio.file.StandardOpenOption.APPEND);
    }

    private void write(final String relative, final String content) throws IOException {
        final Path file = this.project.resolve(relative);
        Files.createDirectories(file.getParent());
        Files.writeString(file, content, StandardCharsets.UTF_8);
    }

    private String read(final String relative) throws IOException {
        return Files.readString(this.project.resolve(relative), StandardCharsets.UTF_8);
    }
}
