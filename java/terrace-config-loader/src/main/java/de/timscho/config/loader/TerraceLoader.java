package de.timscho.config.loader;

import com.fasterxml.jackson.databind.ObjectMapper;
import de.timscho.config.core.descriptor.SchemaAssembler;
import de.timscho.config.core.descriptor.TypeDescriptor;
import de.timscho.config.core.model.LoaderRole;
import de.timscho.config.core.model.LoaderVar;
import de.timscho.config.core.model.Schema;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.SortedSet;
import java.util.TreeSet;
import lombok.AccessLevel;
import lombok.RequiredArgsConstructor;
import lombok.Setter;
import lombok.experimental.Accessors;
import org.jspecify.annotations.Nullable;

/**
 * The layered loader — the Java equivalent of the Rust crate's {@code Terrace}.
 *
 * <p>Layers, lowest precedence first: struct/record defaults (Jackson leaves an absent key at
 * whatever the target's own default is), TOML at {@code $<PREFIX>CONFIG} (a file, or every
 * {@code *.toml} in it when it names a directory), {@code <PREFIX>}-prefixed {@code __}-nested
 * environment variables, {@code $<PREFIX>SECRETS_DIR}, and {@code <PREFIX><KEY>_FILE}
 * indirection.
 *
 * <p>Every environment name is derived from one prefix unless overridden, which is the whole of
 * the parameterisation this loader needs. {@code producer.loader} for a contract built with this
 * loader is {@code terrace-java}.
 *
 * <pre>{@code
 * record Config(Database database) {}
 * record Database(String url) {}
 *
 * Config config = TerraceLoader.of("MYAPP_").reserve("MYAPP_PROFILE").load(Config.class);
 * }</pre>
 */
@Setter
@Accessors(fluent = true, chain = true)
@RequiredArgsConstructor(access = AccessLevel.PRIVATE)
public final class TerraceLoader {

    private static final String DEFAULT_CONFIG_PATH = "config.toml";

    private final String prefix;

    /** Override the variable naming the TOML layer. Defaults to {@code <PREFIX>CONFIG}. */
    private @Nullable String configVar;

    /** Override the variable naming the secrets directory. Defaults to {@code <PREFIX>SECRETS_DIR}. */
    private @Nullable String secretsDirVar;

    /** Where the TOML layer looks when the configuration variable is unset. Defaults to {@code config.toml}. */
    private Path defaultConfigPath = Path.of(DEFAULT_CONFIG_PATH);

    /** Override the indirection suffix. Defaults to {@code _FILE}. */
    private String fileSuffix = "_FILE";

    @Setter(AccessLevel.NONE)
    private String separator = "__";

    private final List<String> reserved = new ArrayList<>();

    /** What to do when one key is supplied by two mechanisms. Defaults to {@link ShadowPolicy#REJECT}. */
    private ShadowPolicy shadowPolicy = ShadowPolicy.REJECT;

    /**
     * A loader over {@code prefix}.
     *
     * <p>{@code TerraceLoader.of("MYAPP_")} reads {@code MYAPP_CONFIG}, {@code
     * MYAPP_SECRETS_DIR}, {@code MYAPP_*} and {@code MYAPP_<KEY>_FILE}. The prefix is taken
     * verbatim, trailing underscore included.
     *
     * @param prefix every environment variable this loader reads, taken verbatim
     */
    public static TerraceLoader of(final String prefix) {
        return new TerraceLoader(prefix);
    }

    /** Override the nesting separator. Defaults to {@code __}.
     *
     * @param newSeparator what separates nesting levels in an environment key
     */
    public TerraceLoader nestingSeparator(final String newSeparator) {
        this.separator = newSeparator;
        return this;
    }

    /**
     * Reserve a key, in its <b>full environment spelling</b>, e.g. {@code MYAPP_PROFILE}. A
     * reserved key is read from the environment before the layers exist, so a file may not
     * supply it.
     *
     * @param key the full environment spelling of the key to reserve
     */
    public TerraceLoader reserve(final String key) {
        this.reserved.add(key);
        return this;
    }

    /** The variable naming the TOML layer. */
    public String configVarName() {
        return this.configVar != null ? this.configVar : this.prefix + "CONFIG";
    }

    /** The variable naming the secrets directory. */
    public String secretsDirVarName() {
        return this.secretsDirVar != null ? this.secretsDirVar : this.prefix + "SECRETS_DIR";
    }

    /** The environment spelling this loader reads, reserved keys included. */
    public Dialect dialect() {
        Dialect dialect = Dialect.of(this.prefix)
                .withNestingSeparator(this.separator)
                .withFileSuffix(this.fileSuffix)
                .reserve(this.configVarName())
                .reserve(this.secretsDirVarName());
        for (final String key : this.reserved) {
            dialect = dialect.reserve(key);
        }
        return dialect;
    }

    /**
     * Every key {@code descriptor} describes, in every spelling this loader accepts.
     *
     * <p>Reads nothing: the answer is a property of the described type and of this loader's own
     * dialect, not of the environment the process happens to be running in — usable from a
     * documentation job, where none of the variables it describes are set. {@code descriptor}
     * is usually a generated {@code <Type>Descriptor.DESCRIPTOR} from {@code
     * terrace-config-processor}.
     *
     * @param descriptor the configuration type's own field-level descriptor
     */
    public Schema schema(final TypeDescriptor descriptor) {
        return this.schemaAt(descriptor, "");
    }

    /**
     * Every key {@code descriptor} describes, as spelled when it sits at {@code root} in a
     * larger configuration — {@code schemaAt(CspDescriptor.DESCRIPTOR, "csp")} produces {@code
     * csp.cloudflare.turnstile}, where {@link #schema} alone would produce {@code
     * cloudflare.turnstile}, a path that appears in no configuration file anywhere.
     *
     * @param descriptor the configuration type's own field-level descriptor
     * @param root       where {@code descriptor} sits in the larger configuration, dotted, empty at the root
     */
    public Schema schemaAt(final TypeDescriptor descriptor, final String root) {
        final de.timscho.config.core.model.Dialect modelDialect = de.timscho.config.core.model.Dialect.builder()
                .prefix(this.prefix)
                .nestingSeparator(this.separator)
                .indirectionSuffix(this.fileSuffix)
                .build();

        final Set<String> reservedEnvNames = new LinkedHashSet<>(this.reserved);
        reservedEnvNames.add(this.configVarName());
        reservedEnvNames.add(this.secretsDirVarName());

        final Schema schema = SchemaAssembler.assemble(descriptor, modelDialect, reservedEnvNames, root);
        final List<LoaderVar> loaderVars = new ArrayList<>();
        loaderVars.add(LoaderVar.builder()
                .env(this.configVarName())
                .role(LoaderRole.CONFIG)
                .docs("Names the TOML layer: a file, or a directory whose *.toml files are all merged in name order.")
                .defaultValue(this.defaultConfigPath.toString())
                .build());
        loaderVars.add(LoaderVar.builder()
                .env(this.secretsDirVarName())
                .role(LoaderRole.SECRETS_DIR)
                .docs("Names a directory of key-named files -- a mounted Kubernetes Secret volume. Each file "
                        + "supplies the key its name spells.")
                .build());
        for (final String reservedKey : this.reserved) {
            loaderVars.add(LoaderVar.builder()
                    .env(reservedKey)
                    .role(LoaderRole.RESERVED)
                    .docs("Read directly from the environment before the layered config exists, so no file "
                            + "may supply it.")
                    .build());
        }
        return schema.toBuilder().loader(loaderVars).build();
    }

    /** Load a typed config, reading {@code System.getenv()} and the configured paths.
     *
     * @param <T>  the configuration type
     * @param type the configuration type's own class, to bind the merged layers against
     */
    public <T> T load(final Class<T> type) {
        return this.load(type, System.getenv());
    }

    /**
     * Load a typed config against an explicit environment map, rather than {@code
     * System.getenv()} — the seam a test uses to run this loader without touching the real
     * process environment.
     *
     * @param <T>         the configuration type
     * @param type        the configuration type's own class, to bind the merged layers against
     * @param environment the environment variables to read, in place of {@code System.getenv()}
     */
    public <T> T load(final Class<T> type, final Map<String, String> environment) {
        final Map<String, Object> merged = this.assemble(environment);
        return new ObjectMapper().convertValue(merged, type);
    }

    /**
     * Load a typed config and everything a reload needs to load it again later, reading {@code
     * System.getenv()} and the configured paths.
     *
     * @param <T>  the configuration type
     * @param type the configuration type's own class, to bind the merged layers against
     */
    public <T> Loaded<T> loadWatched(final Class<T> type) {
        return this.loadWatched(type, System.getenv());
    }

    /**
     * As {@link #loadWatched(Class)}, against an explicit environment map — the seam a test uses
     * to exercise a reload without touching the real process environment.
     *
     * <p>The merged map, before binding, is kept whole as the fingerprint: the typed value may
     * hold a secret in a type with no useful {@code equals} of its own, so comparing the merged
     * map instead is what makes {@link Sources#differsFrom} usable at all.
     *
     * @param <T>         the configuration type
     * @param type        the configuration type's own class, to bind the merged layers against
     * @param environment the environment variables to read, in place of {@code System.getenv()}
     */
    public <T> Loaded<T> loadWatched(final Class<T> type, final Map<String, String> environment) {
        final Layers layers = this.buildLayers(environment);

        final Map<String, Object> merged = new LinkedHashMap<>();
        LayerValues.deepMerge(merged, layers.toml().merged());
        LayerValues.deepMerge(merged, this.environmentLayer(layers.dialect(), environment));
        if (!layers.files().isEmpty()) {
            LayerValues.deepMerge(merged, layers.files().merged());
        }

        final T value = new ObjectMapper().convertValue(merged, type);

        final SortedSet<Path> watch = new TreeSet<>();
        watch.addAll(layers.files().watchPaths());
        layers.toml().watchDir().ifPresent(watch::add);

        return new Loaded<>(value, new Sources(new ArrayList<>(watch), merged));
    }

    /**
     * Where every value this loader can see would come from, reading {@code System.getenv()} and
     * the configured paths.
     *
     * <p>Reports the same layers {@link #load} would bind, without binding them to any type —
     * useful even when {@link #load} itself would refuse (a shadowed key, say), since the report
     * needs no target type to succeed.
     */
    public Explanation explain() {
        return this.explain(System.getenv());
    }

    /** As {@link #explain()}, against an explicit environment map.
     *
     * @param environment the environment variables to read, in place of {@code System.getenv()}
     */
    public Explanation explain(final Map<String, String> environment) {
        return Explanation.of(this.buildLayers(environment), environment);
    }

    /** The merged configuration, before binding to any type — the assembled nested map. */
    Map<String, Object> assemble(final Map<String, String> environment) {
        final Layers layers = this.buildLayers(environment);

        final Map<String, Object> merged = new LinkedHashMap<>();
        LayerValues.deepMerge(merged, layers.toml().merged());
        LayerValues.deepMerge(merged, this.environmentLayer(layers.dialect(), environment));
        if (!layers.files().isEmpty()) {
            LayerValues.deepMerge(merged, layers.files().merged());
        }
        return merged;
    }

    /**
     * Reads every layer's own source (the TOML files/directory, the file-backed layers) without
     * merging them into one map — the step {@link #assemble} and {@link #explain} share, so
     * neither can disagree with the other about what was actually read.
     */
    private Layers buildLayers(final Map<String, String> environment) {
        final Dialect dialect = this.dialect();

        final String configVarName = this.configVarName();
        final String configured = environment.get(configVarName);
        final boolean configFromEnv = configured != null;
        final Path configPath = configFromEnv ? Path.of(configured) : this.defaultConfigPath;
        final TomlLayers toml = TomlLayers.expand(configVarName, configPath);

        final String secretsVarName = this.secretsDirVarName();
        final String secretsDirValue = environment.get(secretsVarName);
        final Optional<Path> secretsDir = (secretsDirValue != null && !secretsDirValue.isBlank())
                ? Optional.of(Path.of(secretsDirValue))
                : Optional.empty();
        final FileLayers files =
                FileLayers.collect(secretsDir, secretsVarName, dialect, this.shadowPolicy, environment);

        return new Layers(dialect, configVarName, configPath, configFromEnv, toml, secretsVarName, secretsDir, files);
    }

    /**
     * Every layer's own source, read but not yet merged — the Java equivalent of the Rust
     * crate's {@code Terrace::assemble} intermediate {@code Layers} struct, shared between
     * {@link #assemble} (which merges it) and {@link Explanation#of} (which reports on it
     * instead).
     */
    record Layers(
            Dialect dialect,
            String configVar,
            Path configPath,
            boolean configFromEnv,
            TomlLayers toml,
            String secretsVar,
            Optional<Path> secretsDir,
            FileLayers files) {}

    /**
     * The environment layer: the prefixed variables that are values, and no others.
     *
     * <p>Every variable under the prefix is one set too many: anything {@link #reserve reserved}
     * (the configuration and secrets-directory variables among them) and every {@code
     * <PREFIX><KEY>_FILE} are this loader's own mechanism rather than configuration — {@link
     * FileLayers} has already read and merged the latter as the key it names.
     */
    private Map<String, Object> environmentLayer(final Dialect dialect, final Map<String, String> environment) {
        final Map<String, Object> dict = new LinkedHashMap<>();
        for (final Map.Entry<String, String> entry : environment.entrySet()) {
            final String name = entry.getKey();
            if (!name.startsWith(this.prefix)) {
                continue;
            }
            if (dialect.isReserved(name) || dialect.indirectionTarget(name).isPresent()) {
                continue;
            }
            final String suffix = name.substring(this.prefix.length());
            if (suffix.isEmpty()) {
                continue;
            }
            LayerValues.insertNested(dict, dialect.keyPath(suffix), entry.getValue());
        }
        return dict;
    }
}
