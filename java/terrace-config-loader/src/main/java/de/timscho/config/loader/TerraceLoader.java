package de.timscho.config.loader;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

import com.fasterxml.jackson.databind.ObjectMapper;

import lombok.AccessLevel;
import lombok.RequiredArgsConstructor;
import lombok.Setter;
import lombok.experimental.Accessors;

import de.timscho.config.core.descriptor.SchemaAssembler;
import de.timscho.config.core.descriptor.TypeDescriptor;
import de.timscho.config.core.model.LoaderRole;
import de.timscho.config.core.model.LoaderVar;
import de.timscho.config.core.model.Schema;

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
    private String configVar;

    /** Override the variable naming the secrets directory. Defaults to {@code <PREFIX>SECRETS_DIR}. */
    private String secretsDirVar;

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
     */
    public static TerraceLoader of(String prefix) {
        return new TerraceLoader(prefix);
    }

    /** Override the nesting separator. Defaults to {@code __}. */
    public TerraceLoader nestingSeparator(String separator) {
        this.separator = separator;
        return this;
    }

    /**
     * Reserve a key, in its <b>full environment spelling</b>, e.g. {@code MYAPP_PROFILE}. A
     * reserved key is read from the environment before the layers exist, so a file may not
     * supply it.
     */
    public TerraceLoader reserve(String key) {
        this.reserved.add(key);
        return this;
    }

    /** The variable naming the TOML layer. */
    public String configVarName() {
        return configVar != null ? configVar : prefix + "CONFIG";
    }

    /** The variable naming the secrets directory. */
    public String secretsDirVarName() {
        return secretsDirVar != null ? secretsDirVar : prefix + "SECRETS_DIR";
    }

    /** The environment spelling this loader reads, reserved keys included. */
    public Dialect dialect() {
        Dialect dialect = Dialect.of(prefix)
                .withNestingSeparator(separator)
                .withFileSuffix(fileSuffix)
                .reserve(configVarName())
                .reserve(secretsDirVarName());
        for (String key : reserved) {
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
     */
    public Schema schema(TypeDescriptor descriptor) {
        return schemaAt(descriptor, "");
    }

    /**
     * Every key {@code descriptor} describes, as spelled when it sits at {@code root} in a
     * larger configuration — {@code schemaAt(CspDescriptor.DESCRIPTOR, "csp")} produces {@code
     * csp.cloudflare.turnstile}, where {@link #schema} alone would produce {@code
     * cloudflare.turnstile}, a path that appears in no configuration file anywhere.
     */
    public Schema schemaAt(TypeDescriptor descriptor, String root) {
        de.timscho.config.core.model.Dialect modelDialect = de.timscho.config.core.model.Dialect.builder()
                .prefix(prefix)
                .nestingSeparator(separator)
                .indirectionSuffix(fileSuffix)
                .build();

        Set<String> reservedEnvNames = new LinkedHashSet<>(reserved);
        reservedEnvNames.add(configVarName());
        reservedEnvNames.add(secretsDirVarName());

        Schema schema = SchemaAssembler.assemble(descriptor, modelDialect, reservedEnvNames, root);
        List<LoaderVar> loaderVars = new ArrayList<>();
        loaderVars.add(LoaderVar.builder()
                .env(configVarName())
                .role(LoaderRole.CONFIG)
                .docs("Names the TOML layer: a file, or a directory whose *.toml files are all merged in name order.")
                .defaultValue(defaultConfigPath.toString())
                .build());
        loaderVars.add(LoaderVar.builder()
                .env(secretsDirVarName())
                .role(LoaderRole.SECRETS_DIR)
                .docs("Names a directory of key-named files -- a mounted Kubernetes Secret volume. Each file supplies the key its name spells.")
                .build());
        for (String reservedKey : reserved) {
            loaderVars.add(LoaderVar.builder()
                    .env(reservedKey)
                    .role(LoaderRole.RESERVED)
                    .docs("Read directly from the environment before the layered config exists, so no file may supply it.")
                    .build());
        }
        return schema.toBuilder().loader(loaderVars).build();
    }

    /** Load a typed config, reading {@code System.getenv()} and the configured paths. */
    public <T> T load(Class<T> type) {
        return load(type, System.getenv());
    }

    /**
     * Load a typed config against an explicit environment map, rather than {@code
     * System.getenv()} — the seam a test uses to run this loader without touching the real
     * process environment.
     */
    public <T> T load(Class<T> type, Map<String, String> environment) {
        Map<String, Object> merged = assemble(environment);
        return new ObjectMapper().convertValue(merged, type);
    }

    /**
     * Load a typed config and everything a reload needs to load it again later, reading {@code
     * System.getenv()} and the configured paths.
     */
    public <T> Loaded<T> loadWatched(Class<T> type) {
        return loadWatched(type, System.getenv());
    }

    /**
     * As {@link #loadWatched(Class)}, against an explicit environment map — the seam a test uses
     * to exercise a reload without touching the real process environment.
     *
     * <p>The merged map, before binding, is kept whole as the fingerprint: the typed value may
     * hold a secret in a type with no useful {@code equals} of its own, so comparing the merged
     * map instead is what makes {@link Sources#differsFrom} usable at all.
     */
    public <T> Loaded<T> loadWatched(Class<T> type, Map<String, String> environment) {
        Layers layers = buildLayers(environment);

        Map<String, Object> merged = new LinkedHashMap<>();
        LayerValues.deepMerge(merged, layers.toml().merged());
        LayerValues.deepMerge(merged, environmentLayer(layers.dialect(), environment));
        if (!layers.files().isEmpty()) {
            LayerValues.deepMerge(merged, layers.files().merged());
        }

        T value = new ObjectMapper().convertValue(merged, type);

        java.util.TreeSet<Path> watch = new java.util.TreeSet<>();
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
        return explain(System.getenv());
    }

    /** As {@link #explain()}, against an explicit environment map. */
    public Explanation explain(Map<String, String> environment) {
        return Explanation.of(buildLayers(environment), environment);
    }

    /** The merged configuration, before binding to any type — the assembled nested map. */
    Map<String, Object> assemble(Map<String, String> environment) {
        Layers layers = buildLayers(environment);

        Map<String, Object> merged = new LinkedHashMap<>();
        LayerValues.deepMerge(merged, layers.toml().merged());
        LayerValues.deepMerge(merged, environmentLayer(layers.dialect(), environment));
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
    private Layers buildLayers(Map<String, String> environment) {
        Dialect dialect = dialect();

        String configVarName = configVarName();
        String configured = environment.get(configVarName);
        boolean configFromEnv = configured != null;
        Path configPath = configFromEnv ? Path.of(configured) : defaultConfigPath;
        TomlLayers toml = TomlLayers.expand(configVarName, configPath);

        String secretsVarName = secretsDirVarName();
        String secretsDirValue = environment.get(secretsVarName);
        Optional<Path> secretsDir = (secretsDirValue != null && !secretsDirValue.isBlank())
                ? Optional.of(Path.of(secretsDirValue))
                : Optional.empty();
        FileLayers files = FileLayers.collect(secretsDir, secretsVarName, dialect, shadowPolicy, environment);

        return new Layers(dialect, configVarName, configPath, configFromEnv, toml, secretsVarName, secretsDir, files);
    }

    /**
     * Every layer's own source, read but not yet merged — the Java equivalent of the Rust
     * crate's {@code Terrace::assemble} intermediate {@code Layers} struct, shared between
     * {@link #assemble} (which merges it) and {@link Explanation#of} (which reports on it
     * instead).
     */
    record Layers(
            Dialect dialect, String configVar, Path configPath, boolean configFromEnv, TomlLayers toml,
            String secretsVar, Optional<Path> secretsDir, FileLayers files) {
    }

    /**
     * The environment layer: the prefixed variables that are values, and no others.
     *
     * <p>Every variable under the prefix is one set too many: anything {@link #reserve reserved}
     * (the configuration and secrets-directory variables among them) and every {@code
     * <PREFIX><KEY>_FILE} are this loader's own mechanism rather than configuration — {@link
     * FileLayers} has already read and merged the latter as the key it names.
     */
    private Map<String, Object> environmentLayer(Dialect dialect, Map<String, String> environment) {
        Map<String, Object> dict = new LinkedHashMap<>();
        for (Map.Entry<String, String> entry : environment.entrySet()) {
            String name = entry.getKey();
            if (!name.startsWith(prefix)) {
                continue;
            }
            if (dialect.isReserved(name) || dialect.indirectionTarget(name).isPresent()) {
                continue;
            }
            String suffix = name.substring(prefix.length());
            if (suffix.isEmpty()) {
                continue;
            }
            LayerValues.insertNested(dict, dialect.keyPath(suffix), entry.getValue());
        }
        return dict;
    }
}
