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
    static Explanation of(TerraceLoader.Layers layers, Map<String, String> environment) {
        // Appended in merge order, so the last source of a key is the one in effect. The order
        // below is `TerraceLoader.assemble`'s own, and has to stay it.
        Map<String, List<Layer>> sources = new TreeMap<>();

        List<Map.Entry<Path, Fragment>> fragments = new ArrayList<>();
        for (Path path : layers.toml().files()) {
            Fragment fragment;
            if (!Files.isRegularFile(path)) {
                fragment = new Fragment.Missing();
            } else {
                List<String> keys = TomlLayers.fragmentKeys(path);
                if (keys == null) {
                    fragment = new Fragment.Unreadable();
                } else {
                    fragment = new Fragment.Read(keys.size());
                    for (String key : keys) {
                        sources.computeIfAbsent(key, k -> new ArrayList<>()).add(new Layer.Toml(path));
                    }
                }
            }
            fragments.add(Map.entry(path, fragment));
        }

        Map<String, Set<String>> env = layers.dialect().plainEnvEntries(environment);
        int envKeys = env.size();
        for (Map.Entry<String, Set<String>> entry : env.entrySet()) {
            for (String var : entry.getValue()) {
                sources.computeIfAbsent(entry.getKey(), k -> new ArrayList<>()).add(new Layer.Env(var));
            }
        }

        SecretsDir secrets = layers.files().secrets();
        int secretsKeys = secrets == null ? 0 : secrets.values().size();
        if (secrets != null) {
            for (Map.Entry<String, FileValue> entry : secrets.values().entrySet()) {
                sources.computeIfAbsent(entry.getKey(), k -> new ArrayList<>())
                        .add(new Layer.SecretsFile(entry.getValue().path()));
            }
        }

        FileSuffixEnv indirections = layers.files().indirections();
        int indirectionKeys = indirections.values().size();
        for (Map.Entry<String, FileValue> entry : indirections.values().entrySet()) {
            String key = entry.getKey();
            // Recorded alongside the value, so this is the spelling the operator used. The
            // fallback reconstructs the documented one and cannot really be reached: it is the
            // last code that should throw to say so.
            String var = indirections
                    .origin(key)
                    .orElseGet(() ->
                            layers.dialect().envSpelling(key) + layers.dialect().indirectionSuffix());
            sources.computeIfAbsent(key, k -> new ArrayList<>())
                    .add(new Layer.Indirection(var, entry.getValue().path()));
        }

        List<Origin> origins = new ArrayList<>();
        for (Map.Entry<String, List<Layer>> entry : sources.entrySet()) {
            Origin origin = Origin.fromSources(entry.getKey(), entry.getValue());
            if (origin != null) {
                origins.add(origin);
            }
        }

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

    /** One key's origin, by key path ({@code auth.jwt_secret}). */
    public Optional<Origin> origin(String key) {
        for (Origin origin : origins) {
            if (origin.key().equals(key)) {
                return Optional.of(origin);
            }
        }
        return Optional.empty();
    }

    /** The keys more than one layer supplied, in key-path order. */
    public List<Origin> contested() {
        List<Origin> contested = new ArrayList<>();
        for (Origin origin : origins) {
            if (origin.isContested()) {
                contested.add(origin);
            }
        }
        return contested;
    }

    /** The secrets directory in use, if one was configured. */
    public Optional<Path> secretsDir() {
        return Optional.ofNullable(secretsDir);
    }

    /**
     * The whole report, as the class documentation shows it.
     *
     * <p>No trailing newline, so a logging call and {@code System.out.println} both do the right
     * thing.
     */
    @Override
    public String toString() {
        StringBuilder out = new StringBuilder();
        int contestedCount = contested().size();
        out.append("terrace-config: prefix `").append(prefix).append("`, ").append(plural(origins.size(), "key"));
        if (contestedCount > 0) {
            out.append(", ").append(contestedCount).append(" supplied by more than one layer");
        }

        out.append("\nlayers, lowest precedence first:\n  TOML          ");
        if (configFromEnv) {
            out.append(configVar).append('=').append(configPath);
        } else {
            // The path alone would read as though the variable were set to it, and "the
            // variable is not set" is a different thing to check than "the file is not there".
            out.append(configVar).append(" unset, default ").append(configPath);
        }
        for (Map.Entry<Path, Fragment> fragment : fragments) {
            out.append("\n                  ")
                    .append(fragment.getKey())
                    .append(" (")
                    .append(fragment.getValue())
                    .append(')');
        }

        out.append("\n  environment   ")
                .append(prefix)
                .append("* (")
                .append(count(envKeys))
                .append(')');

        out.append("\n  secrets dir   ");
        if (secretsDir != null) {
            out.append(secretsVar)
                    .append('=')
                    .append(secretsDir)
                    .append(" (")
                    .append(count(secretsKeys))
                    .append(')');
        } else {
            out.append(secretsVar).append(" unset");
        }

        out.append("\n  indirection   ")
                .append(prefix)
                .append('*')
                .append(indirectionSuffix)
                .append(" (")
                .append(count(indirectionKeys))
                .append(')');

        out.append("\nkeys:");
        if (origins.isEmpty()) {
            // An empty section reads as a rendering bug; this reads as the finding it is.
            return out.append("\n  none — every value in this configuration is a default")
                    .toString();
        }

        int longest = 0;
        for (Origin origin : origins) {
            longest = Math.max(longest, origin.key().length());
        }
        int width = Math.min(longest, MAX_KEY_WIDTH);

        for (Origin origin : origins) {
            out.append("\n  ").append(pad(origin.key(), width)).append("  <- ").append(origin.effective());
            for (Layer shadowed : origin.shadowed()) {
                out.append("\n  ")
                        .append(pad("", width))
                        .append("     shadowing ")
                        .append(shadowed);
            }
        }
        return out.toString();
    }

    private static String pad(String value, int width) {
        if (value.length() >= width) {
            return value;
        }
        StringBuilder padded = new StringBuilder(value);
        while (padded.length() < width) {
            padded.append(' ');
        }
        return padded.toString();
    }

    /** {@code 1 key} / {@code 4 keys}, so a report reads as English rather than as a template. */
    private static String plural(int count, String noun) {
        return count == 1 ? count + " " + noun : count + " " + noun + "s";
    }

    /** A layer's key count, where zero is worth spelling out: it is the finding, not the absence of one. */
    private static String count(int keys) {
        return keys == 0 ? "none" : plural(keys, "key");
    }
}
