package de.timscho.config.gradle;

import java.io.File;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.HashSet;
import java.util.LinkedHashSet;
import java.util.Set;
import org.gradle.api.GradleException;
import org.gradle.api.Plugin;
import org.gradle.api.Project;
import org.gradle.api.artifacts.Configuration;
import org.gradle.api.artifacts.ModuleVersionIdentifier;
import org.gradle.api.artifacts.result.DependencyResult;
import org.gradle.api.artifacts.result.ResolvedComponentResult;
import org.gradle.api.artifacts.result.ResolvedDependencyResult;
import org.gradle.api.attributes.Bundling;
import org.gradle.api.attributes.Category;
import org.gradle.api.attributes.LibraryElements;
import org.gradle.api.attributes.Usage;
import org.gradle.api.attributes.java.TargetJvmEnvironment;
import org.gradle.api.file.RegularFile;
import org.gradle.api.model.ObjectFactory;
import org.gradle.api.plugins.JavaPlugin;
import org.gradle.api.plugins.JavaPluginExtension;
import org.gradle.api.provider.Provider;
import org.gradle.api.provider.ProviderFactory;
import org.gradle.api.tasks.SourceSet;
import org.gradle.api.tasks.TaskProvider;
import org.gradle.jvm.toolchain.JavaToolchainService;
import org.gradle.language.base.plugins.LifecycleBasePlugin;
import org.jspecify.annotations.Nullable;

/**
 * {@code de.timscho.terrace-config}: emits a service's configuration contract from its own types
 * at build time, stamps and conforms it with {@code terrace-contract}, and keeps the committed copy
 * honest. Applies {@code java}.
 *
 * <ul>
 *   <li>{@code terraceContractEmit}: the unstamped document, from {@code ContractMain} or {@code
 *       SpringContractMain} on the runtime classpath. Cacheable.
 *   <li>{@code terraceContract}: that document stamped with the build's identity and conformed at
 *       the producer's tier, written to {@code build/terrace/contract.json}.
 *   <li>{@code terraceContractCheck}: fails when the committed {@code contract.json} differs from
 *       the unstamped emission. {@code check} depends on it.
 *   <li>{@code terraceContractUpdate}: rewrites the committed copy.
 * </ul>
 *
 * <p>The {@code terraceContractGenerator} configuration adds dependencies the entry point needs
 * and the service does not ship: {@code SpringContractMain} needs {@code
 * terrace-config-core-jackson2} there, since {@code terrace-config-spring-boot} deliberately brings
 * no Jackson of its own.
 */
public final class TerraceContractPlugin implements Plugin<Project> {

    /** The extension's name, and the stamped values' Gradle property namespace. */
    static final String EXTENSION = "terraceContract";

    /** The configuration for the entry point's own dependencies. */
    static final String GENERATOR = "terraceContractGenerator";

    private static final String GENERATOR_CLASSPATH = "terraceContractGeneratorClasspath";

    private static final String GROUP = "terrace contract";

    /** Creates the plugin. Gradle instantiates it when a build applies it. */
    public TerraceContractPlugin() {
        // Stateless: everything it configures hangs off the project passed to apply.
    }

    @Override
    public void apply(final Project project) {
        project.getPluginManager().apply(JavaPlugin.class);
        final ProviderFactory providers = project.getProviders();
        final TerraceContractExtension extension =
                project.getExtensions().create(EXTENSION, TerraceContractExtension.class);
        conventions(project, extension);

        final Configuration classpath = generatorClasspath(project);
        final Provider<ContractLoader> loader = extension
                .getLoader()
                .map(ContractLoader::byId)
                .orElse(classpath
                        .getIncoming()
                        .getResolutionResult()
                        .getRootComponent()
                        .map(TerraceContractPlugin::detect));
        extension.getTier().convention(loader.map(ContractLoader::tier));

        final SourceSet main = project.getExtensions()
                .getByType(JavaPluginExtension.class)
                .getSourceSets()
                .getByName(SourceSet.MAIN_SOURCE_SET_NAME);
        final JavaToolchainService toolchains = project.getExtensions().getByType(JavaToolchainService.class);
        final JavaPluginExtension java = project.getExtensions().getByType(JavaPluginExtension.class);

        final TaskProvider<TerraceContractEmit> emit = project.getTasks()
                .register("terraceContractEmit", TerraceContractEmit.class, task -> {
                    task.setGroup(GROUP);
                    task.setDescription("Emits this service's configuration contract, unstamped, from its types.");
                    task.getClasspath().from(main.getOutput(), classpath);
                    task.getMainClass().set(loader.map(ContractLoader::mainClass));
                    task.getType().set(extension.getType());
                    task.getPrefix().set(extension.getPrefix());
                    task.getAppName().set(extension.getAppName());
                    task.getReserve().set(extension.getReserve());
                    task.getProducerArguments().set(extension.getProducerArguments());
                    task.getJavaLauncher().convention(toolchains.launcherFor(java.getToolchain()));
                    task.getOutput()
                            .set(project.getLayout().getBuildDirectory().file("terrace/contract.unstamped.json"));
                });
        final Provider<RegularFile> emitted = emit.flatMap(TerraceContractEmit::getOutput);

        final File rootDir = project.getRootDir();
        final Provider<String> path = providers.environmentVariable("PATH");
        final Provider<String> explicit = providers.environmentVariable(ExecutableLocator.BIN_VARIABLE);
        final String osName = providers.systemProperty("os.name").getOrElse("");
        project.getTasks().register("terraceContract", TerraceContractStamp.class, task -> {
            task.setGroup(GROUP);
            task.setDescription(
                    "Stamps this service's configuration contract with the build's identity and conforms it.");
            task.getUnstamped().set(emitted);
            task.getExecutable()
                    .set(extension
                            .getExecutable()
                            .orElse(project.getLayout()
                                    .file(providers.provider(() -> ExecutableLocator.locate(
                                            explicit.getOrNull(), rootDir, path.getOrNull(), osName)))));
            task.getSearched().set(providers.provider(() -> ExecutableLocator.searched(path.getOrNull(), osName)));
            task.getTier().set(extension.getTier());
            task.getAppVersion().set(extension.getAppVersion());
            task.getRevision().set(extension.getRevision());
            task.getCreated().set(extension.getCreated());
            task.getSource().set(extension.getSource());
            task.getOutput().set(extension.getOutput());
        });

        final TaskProvider<TerraceContractUpdate> update = project.getTasks()
                .register("terraceContractUpdate", TerraceContractUpdate.class, task -> {
                    task.setGroup(GROUP);
                    task.setDescription("Rewrites the committed contract with what the types emit today.");
                    task.getEmitted().set(emitted);
                    task.getCommitted().set(extension.getCommittedContract());
                });
        final String updatePath = (":".equals(project.getPath()) ? "" : project.getPath()) + ":" + update.getName();
        final TaskProvider<TerraceContractCheck> check = project.getTasks()
                .register("terraceContractCheck", TerraceContractCheck.class, task -> {
                    task.setGroup(LifecycleBasePlugin.VERIFICATION_GROUP);
                    task.setDescription("Fails when the committed contract differs from what the types emit.");
                    task.getEmitted().set(emitted);
                    task.getCommitted().set(extension.getCommittedContract());
                    task.getUpdateTaskPath().set(updatePath);
                    task.mustRunAfter(update);
                });
        project.getTasks().named(LifecycleBasePlugin.CHECK_TASK_NAME).configure(task -> task.dependsOn(check));
    }

    private static void conventions(final Project project, final TerraceContractExtension extension) {
        final ProviderFactory providers = project.getProviders();
        extension.getAppName().convention(project.getName());
        extension
                .getAppVersion()
                .convention(stamped(providers, "appVersion", null)
                        .orElse(providers.provider(() -> projectVersion(project))));
        extension.getRevision().convention(stamped(providers, "revision", "TERRACE_CONTRACT_REVISION"));
        extension.getCreated().convention(stamped(providers, "created", "TERRACE_CONTRACT_CREATED"));
        extension.getOutput().convention(project.getLayout().getBuildDirectory().file("terrace/contract.json"));
        extension
                .getCommittedContract()
                .convention(project.getLayout().getProjectDirectory().file("contract.json"));
    }

    /** {@code -PterraceContract.<name>}, then {@code variable} when there is one. */
    private static Provider<String> stamped(
            final ProviderFactory providers, final String name, final @Nullable String variable) {
        final Provider<String> property = providers.gradleProperty(EXTENSION + "." + name);
        return variable == null ? property : property.orElse(providers.environmentVariable(variable));
    }

    /** The project's version, or {@code null} while it is Gradle's {@code unspecified} placeholder. */
    private static @Nullable String projectVersion(final Project project) {
        final String version = String.valueOf(project.getVersion());
        return Project.DEFAULT_VERSION.equals(version) ? null : version;
    }

    /**
     * The entry point's classpath: everything the service runs with, plus {@value #GENERATOR}, as
     * one graph, so a dependency both name is resolved once to one version.
     */
    private static Configuration generatorClasspath(final Project project) {
        final Configuration generator = project.getConfigurations().create(GENERATOR, configuration -> {
            configuration.setCanBeConsumed(false);
            configuration.setCanBeResolved(false);
            configuration.setDescription("Dependencies the contract's build-time entry point needs and the service"
                    + " does not ship, such as terrace-config-core-jackson2 for SpringContractMain.");
        });
        final ObjectFactory objects = project.getObjects();
        return project.getConfigurations().create(GENERATOR_CLASSPATH, configuration -> {
            configuration.setCanBeConsumed(false);
            configuration.setCanBeResolved(true);
            configuration.setDescription("The classpath terraceContractEmit runs the entry point on.");
            configuration.extendsFrom(
                    project.getConfigurations().getByName(JavaPlugin.IMPLEMENTATION_CONFIGURATION_NAME),
                    project.getConfigurations().getByName(JavaPlugin.RUNTIME_ONLY_CONFIGURATION_NAME),
                    generator);
            configuration.attributes(attributes -> {
                attributes.attribute(Usage.USAGE_ATTRIBUTE, objects.named(Usage.class, Usage.JAVA_RUNTIME));
                attributes.attribute(Category.CATEGORY_ATTRIBUTE, objects.named(Category.class, Category.LIBRARY));
                attributes.attribute(
                        LibraryElements.LIBRARY_ELEMENTS_ATTRIBUTE,
                        objects.named(LibraryElements.class, LibraryElements.JAR));
                attributes.attribute(Bundling.BUNDLING_ATTRIBUTE, objects.named(Bundling.class, Bundling.EXTERNAL));
                attributes.attribute(
                        TargetJvmEnvironment.TARGET_JVM_ENVIRONMENT_ATTRIBUTE,
                        objects.named(TargetJvmEnvironment.class, TargetJvmEnvironment.STANDARD_JVM));
            });
        });
    }

    /**
     * The producer whose module is in the resolved graph.
     *
     * @throws GradleException when both or neither is, asking for {@code loader} to be set
     */
    static ContractLoader detect(final ResolvedComponentResult root) {
        final Set<ContractLoader> found = new LinkedHashSet<>();
        final Set<ResolvedComponentResult> seen = new HashSet<>();
        final Deque<ResolvedComponentResult> pending = new ArrayDeque<>();
        pending.add(root);
        while (!pending.isEmpty()) {
            final ResolvedComponentResult component = pending.poll();
            if (!seen.add(component)) {
                continue;
            }
            final ModuleVersionIdentifier module = component.getModuleVersion();
            final ContractLoader loader = module == null ? null : ContractLoader.byModule(module.getName());
            if (loader != null) {
                found.add(loader);
            }
            for (final DependencyResult dependency : component.getDependencies()) {
                if (dependency instanceof ResolvedDependencyResult resolved) {
                    pending.add(resolved.getSelected());
                }
            }
        }
        if (found.size() == 1) {
            return found.iterator().next();
        }
        throw new GradleException((found.isEmpty()
                        ? "Neither terrace-config-loader nor terrace-config-spring-boot is on the runtime classpath"
                        : "Both terrace-config-loader and terrace-config-spring-boot are on the runtime classpath")
                + ", so the contract's producer cannot be inferred. Set `terraceContract { loader = \"terrace-java\" }`"
                + " or `loader = \"spring-boot\"`.");
    }
}
