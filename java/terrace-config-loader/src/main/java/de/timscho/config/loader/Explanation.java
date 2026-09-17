package de.timscho.config.loader;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeMap;
import lombok.AccessLevel;
import lombok.AllArgsConstructor;
import lombok.Getter;
import lombok.experimental.Accessors;
import org.jspecify.annotations.Nullable;

/**
 * Where every value {@link TerraceLoader} can see would come from — the Java equivalent of the
 * Rust crate's {@code explain::Explanation}, built by {@link TerraceLoader#explain()}.
 *
 * <pre>{@code
 * terrace-config: prefix `MYAPP_`, 4 keys, 1 supplied by more than one layer
 * layers, lowest precedence first:
 *   TOML          MYAPP_CONFIG=/etc/myapp/conf.d
 *                   /etc/myapp/conf.d/10-base.toml (2 keys)
 *                   /etc/myapp/conf.d/20-tuning.toml (1 key)
 *   environment   MYAPP_* (1 key)
 *   secrets dir   MYAPP_SECRETS_DIR=/run/secrets (1 key)
 *   indirection   MYAPP_*_FILE (none)
 * keys:
 *   auth.jwt_secret  <- secrets file /run/secrets/auth__jwt_secret
 *   database.url     <- environment MYAPP_DATABASE__URL
 *                       shadowing TOML /etc/myapp/conf.d/10-base.toml
 *   server.port      <- TOML /etc/myapp/conf.d/20-tuning.toml
 *   server.workers   <- TOML /etc/myapp/conf.d/10-base.toml
 * }</pre>
 *
 * <p><b>Nothing here holds a value.</b> An {@link Explanation} records <i>where</i> each key came
 * from and never <i>what</i> it was — there is no field to leak, so {@link #toString()} is safe
 * to log by construction rather than by remembering to redact. A TOML fragment that will not
 * parse is reported as {@link Fragment.Unreadable} with no reason attached for the same reason: a
 * parse error quotes the line it failed on, and that line can be the credential.
 * {@link TerraceLoader#load(Class, Map)} fails with the parser's own message, which is where the
 * detail belongs.
 */
@Accessors(fluent = true)
@AllArgsConstructor(access = AccessLevel.PRIVATE)
public final class Explanation {

    /** The longest key a rendered line pads to, so one absurd path does not indent every other. */
    private static final int MAX_KEY_WIDTH = 44;

    /** The prefix every variable in this report derives from. */
    @Getter
    private final String prefix;

    private final String indirectionSuffix;
    private final String configVar;
    private final Path configPath;
    private final boolean configFromEnv;

    /** Every file the TOML layer expanded to, in merge order, and what became of it. */
    @Getter
    private final List<Map.Entry<Path, Fragment>> fragments;

    private final String secretsVar;
    private final @Nullable Path secretsDir;
    private final int envKeys;
    private final int secretsKeys;
    private final int indirectionKeys;

    /**
     * Every key some layer supplied, in key-path order.
     *
     * <p>Keys nothing supplied are absent: this reports what the <i>environment</i> did, not what
     * the configuration type can carry. {@link TerraceLoader#schema} answers the other half, and
     * answers it without reading anything.
     */
    @Getter
    private final List<Origin> origins;

    /**
     * Report on already-collected layers.
     *
     * <p>Package-private, and takes the loader's own assembly product rather than re-reading the
     * environment: a report that did its own reading could disagree with the load it describes.
     */
    static Explanation of(final TerraceLoader.Layers layers, final Map<String, String> environment) {
        // Appended in merge order, so the last source of a key is the one in effect. The order
        // below is `TerraceLoader.assemble`'s own, and has to stay it.
        final Map<String, List<Layer>> sources = new TreeMap<>();

        final List<Map.Entry<Path, Fragment>> fragments = collectTomlFragments(layers, sources);
        final int envKeys = collectEnvSources(layers, environment, sources);
        final int secretsKeys = collectSecretsSources(layers, sources);
        final int indirectionKeys = collectIndirectionSources(layers, sources);
        final List<Origin> origins = collectOrigins(sources);

        return new Explanation(
                layers.dialect().prefix(),
                layers.dialect().indirectionSuffix(),
                layers.configVar(),
                layers.configPath(),
                layers.configFromEnv(),
                fragments,
                layers.secretsVar(),
                layers.secretsDir().orElse(null),
                envKeys,
                secretsKeys,
                indirectionKeys,
                origins);
    }

    /** Every TOML layer's own read result, recording each key it supplied into {@code sources}. */
    private static List<Map.Entry<Path, Fragment>> collectTomlFragments(
            final TerraceLoader.Layers layers, final Map<String, List<Layer>> sources) {
        final List<Map.Entry<Path, Fragment>> fragments = new ArrayList<>();
        for (final Path path : layers.toml().files()) {
            final Fragment fragment;
            if (!Files.isRegularFile(path)) {
                fragment = new Fragment.Missing();
            } else {
                final List<String> keys = TomlLayers.fragmentKeys(path);
                if (keys == null) {
                    fragment = new Fragment.Unreadable();
                } else {
                    fragment = new Fragment.Read(keys.size());
                    for (final String key : keys) {
                        sources.computeIfAbsent(key, k -> new ArrayList<>()).add(new Layer.Toml(path));
                    }
                }
            }
            fragments.add(Map.entry(path, fragment));
        }
        return fragments;
    }

    /** The plain (non-file) environment variables that supplied a key, recorded into {@code sources}. */
    private static int collectEnvSources(
            final TerraceLoader.Layers layers,
            final Map<String, String> environment,
            final Map<String, List<Layer>> sources) {
        final Map<String, Set<String>> env = layers.dialect().plainEnvEntries(environment);
        for (final Map.Entry<String, Set<String>> entry : env.entrySet()) {
            for (final String var : entry.getValue()) {
                sources.computeIfAbsent(entry.getKey(), k -> new ArrayList<>()).add(new Layer.Env(var));
            }
        }
        return env.size();
    }

    /** The secrets directory's own values, recorded into {@code sources}. */
    private static int collectSecretsSources(
            final TerraceLoader.Layers layers, final Map<String, List<Layer>> sources) {
        final SecretsDir secrets = layers.files().secrets();
        if (secrets == null) {
            return 0;
        }
        for (final Map.Entry<String, FileValue> entry : secrets.values().entrySet()) {
            sources.computeIfAbsent(entry.getKey(), k -> new ArrayList<>())
                    .add(new Layer.SecretsFile(entry.getValue().path()));
        }
        return secrets.values().size();
    }

    /** Every indirection (a {@code _FILE}-suffixed variable) that supplied a key, recorded into
     * {@code sources}. */
    private static int collectIndirectionSources(
            final TerraceLoader.Layers layers, final Map<String, List<Layer>> sources) {
        final FileSuffixEnv indirections = layers.files().indirections();
        for (final Map.Entry<String, FileValue> entry : indirections.values().entrySet()) {
            final String key = entry.getKey();
            // Recorded alongside the value, so this is the spelling the operator used. The
            // fallback reconstructs the documented one and cannot really be reached: it is the
            // last code that should throw to say so.
            final String var = indirections
                    .origin(key)
                    .orElseGet(() ->
                            layers.dialect().envSpelling(key) + layers.dialect().indirectionSuffix());
            sources.computeIfAbsent(key, k -> new ArrayList<>())
                    .add(new Layer.Indirection(var, entry.getValue().path()));
        }
        return indirections.values().size();
    }

    /** Every key that has at least one source, as its own {@link Origin}. */
    private static List<Origin> collectOrigins(final Map<String, List<Layer>> sources) {
        final List<Origin> origins = new ArrayList<>();
        for (final Map.Entry<String, List<Layer>> entry : sources.entrySet()) {
            final Origin origin = Origin.fromSources(entry.getKey(), entry.getValue());
            if (origin != null) {
                origins.add(origin);
            }
        }
        return origins;
    }

    /** One key's origin, by key path ({@code auth.jwt_secret}).
     *
     * @param key the key path to look up, dotted
     */
    public Optional<Origin> origin(final String key) {
        for (final Origin origin : this.origins) {
            if (origin.key().equals(key)) {
                return Optional.of(origin);
            }
        }
        return Optional.empty();
    }

    /** The keys more than one layer supplied, in key-path order. */
    public List<Origin> contested() {
        final List<Origin> contested = new ArrayList<>();
        for (final Origin origin : this.origins) {
            if (origin.isContested()) {
                contested.add(origin);
            }
        }
        return contested;
    }

    /** The secrets directory in use, if one was configured. */
    public Optional<Path> secretsDir() {
        return Optional.ofNullable(this.secretsDir);
    }

    /**
     * The whole report, as the class documentation shows it.
     *
     * <p>No trailing newline, so a logging call and {@code System.out.println} both do the right
     * thing.
     */
    @Override
    public String toString() {
        final StringBuilder out = new StringBuilder();
        final int contestedCount = this.contested().size();
        out.append("terrace-config: prefix `")
                .append(this.prefix)
                .append("`, ")
                .append(plural(this.origins.size(), "key"));
        if (contestedCount > 0) {
            out.append(", ").append(contestedCount).append(" supplied by more than one layer");
        }

        out.append("\nlayers, lowest precedence first:\n  TOML          ");
        if (this.configFromEnv) {
            out.append(this.configVar).append('=').append(this.configPath);
        } else {
            // The path alone would read as though the variable were set to it, and "the
            // variable is not set" is a different thing to check than "the file is not there".
            out.append(this.configVar).append(" unset, default ").append(this.configPath);
        }
        for (final Map.Entry<Path, Fragment> fragment : this.fragments) {
            out.append("\n                  ")
                    .append(fragment.getKey())
                    .append(" (")
                    .append(fragment.getValue())
                    .append(')');
        }

        out.append("\n  environment   ")
                .append(this.prefix)
                .append("* (")
                .append(count(this.envKeys))
                .append(')');

        out.append("\n  secrets dir   ");
        if (this.secretsDir != null) {
            out.append(this.secretsVar)
                    .append('=')
                    .append(this.secretsDir)
                    .append(" (")
                    .append(count(this.secretsKeys))
                    .append(')');
        } else {
            out.append(this.secretsVar).append(" unset");
        }

        out.append("\n  indirection   ")
                .append(this.prefix)
                .append('*')
                .append(this.indirectionSuffix)
                .append(" (")
                .append(count(this.indirectionKeys))
                .append(')');

        out.append("\nkeys:");
        if (this.origins.isEmpty()) {
            // An empty section reads as a rendering bug; this reads as the finding it is.
            return out.append("\n  none — every value in this configuration is a default")
                    .toString();
        }

        int longest = 0;
        for (final Origin origin : this.origins) {
            longest = Math.max(longest, origin.key().length());
        }
        final int width = Math.min(longest, MAX_KEY_WIDTH);

        for (final Origin origin : this.origins) {
            out.append("\n  ").append(pad(origin.key(), width)).append("  <- ").append(origin.effective());
            for (final Layer shadowed : origin.shadowed()) {
                out.append("\n  ")
                        .append(pad("", width))
                        .append("     shadowing ")
                        .append(shadowed);
            }
        }
        return out.toString();
    }

    private static String pad(final String value, final int width) {
        if (value.length() >= width) {
            return value;
        }
        final StringBuilder padded = new StringBuilder(value);
        while (padded.length() < width) {
            padded.append(' ');
        }
        return padded.toString();
    }

    /** {@code 1 key} / {@code 4 keys}, so a report reads as English rather than as a template. */
    private static String plural(final int count, final String noun) {
        return count == 1 ? count + " " + noun : count + " " + noun + "s";
    }

    /** A layer's key count, where zero is worth spelling out: it is the finding, not the absence of one. */
    private static String count(final int keys) {
        return keys == 0 ? "none" : plural(keys, "key");
    }
}
