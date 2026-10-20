package de.timscho.config.loader;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import de.timscho.config.core.contract.ConfigurationType;
import de.timscho.config.core.contract.ContractAssembler;
import de.timscho.config.core.contract.ProducerArguments;
import de.timscho.config.core.contract.ProducerCommandLine;
import de.timscho.config.core.contract.ProducerIdentity;
import de.timscho.config.core.io.ContractCodec;
import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Schema;
import java.nio.file.Path;
import java.util.Map;
import java.util.Set;

/**
 * Emits the contract of a service configured through {@link TerraceLoader}, at build time, without
 * starting the service: the entry point the {@code de.timscho.terrace-config} Gradle plugin's
 * {@code terraceContract} task runs on the service's own runtime classpath.
 *
 * <pre>{@code
 * java -cp <runtime classpath> de.timscho.config.loader.ContractMain \
 *     --type com.example.Config --prefix MYAPP_ --app-name my-service \
 *     --reserve MYAPP_PROFILE --out build/terrace/contract.json
 * }</pre>
 *
 * <p>Every option that changes what the loader reads changes the document, so each of {@link
 * TerraceLoader}'s schema-relevant settings has an option here; a service passes the same values
 * it boots with. Defaults come from the type itself: a default-constructed instance, converted the
 * way {@link TerraceLoader#load} binds, is exactly what a boot with no configuration at all
 * resolves to. Build identity ({@code app.version}, {@code revision}, {@code created}, {@code
 * source}) is deliberately not an option: it is {@code terrace-contract stamp}'s, applied to the
 * emitted document, so the emission stays a pure function of the types.
 *
 * <p>Exits {@code 0} when the document was written, {@code 1} when it was refused, {@code 2} on a
 * usage or classpath error — see {@link ProducerCommandLine}.
 */
public final class ContractMain {

    /** What {@code --help} prints. */
    static final String USAGE = """
            Usage: de.timscho.config.loader.ContractMain
                       --type <class> --prefix <PREFIX_> --app-name <name> [options]

            Emits the contract of a configuration type loaded through terrace-config-loader.

              --type <class>                the root @TerraceConfig type, by binary name
                                            (com.example.Config, com.example.Outer$Inner)
              --prefix <PREFIX_>            the environment prefix, taken verbatim
              --app-name <name>             the contract's app.name
              --reserve <NAME>              a reserved key, in its full environment spelling;
                                            repeatable
              --nesting-separator <sep>     the nesting separator (default __)
              --file-suffix <suffix>        the indirection suffix (default _FILE)
              --config-var <NAME>           the variable naming the TOML layer
                                            (default <PREFIX_>CONFIG)
              --secrets-dir-var <NAME>      the variable naming the secrets directory
                                            (default <PREFIX_>SECRETS_DIR)
              --default-config-path <path>  where the TOML layer looks when the variable is
                                            unset (default config.toml)
              --out <file>                  write the document here instead of standard output

            Exit status: 0 written, 1 refused, 2 usage or classpath error.
            """;

    private static final String LOADER = "terrace-java";

    private ContractMain() {}

    /**
     * Emits the contract and exits with its status.
     *
     * @param args see {@link #USAGE}
     */
    // System.exit is the only way a `main` reports a status to the build that ran it; it is called
    // only on failure, so a successful run returns normally.
    public static void main(final String[] args) {
        final int status = commandLine().run(args, System.out, System.err);
        if (status != ProducerCommandLine.OK) {
            System.exit(status);
        }
    }

    static ProducerCommandLine commandLine() {
        return new ProducerCommandLine(
                USAGE,
                Set.of(
                        "type",
                        "prefix",
                        "app-name",
                        "nesting-separator",
                        "file-suffix",
                        "config-var",
                        "secrets-dir-var",
                        "default-config-path"),
                Set.of("reserve"),
                ContractMain::produce,
                ContractCodec::write);
    }

    static Contract produce(final ProducerArguments arguments) {
        final ConfigurationType type =
                ConfigurationType.named(arguments.required("type"), ContractMain.class.getClassLoader());
        final Schema schema = loader(arguments).schema(type.descriptor()).withDefaultsFromValue(defaultsOf(type));
        final App app = App.builder().name(arguments.required("app-name")).build();
        return ContractAssembler.assemble(schema, app, ProducerIdentity.forLoader(LOADER));
    }

    private static TerraceLoader loader(final ProducerArguments arguments) {
        final TerraceLoader loader = TerraceLoader.of(arguments.required("prefix"));
        for (final String reserved : arguments.all("reserve")) {
            loader.reserve(reserved);
        }
        final String separator = arguments.optional("nesting-separator");
        if (separator != null) {
            loader.nestingSeparator(separator);
        }
        final String suffix = arguments.optional("file-suffix");
        if (suffix != null) {
            loader.fileSuffix(suffix);
        }
        final String configVar = arguments.optional("config-var");
        if (configVar != null) {
            loader.configVar(configVar);
        }
        final String secretsDirVar = arguments.optional("secrets-dir-var");
        if (secretsDirVar != null) {
            loader.secretsDirVar(secretsDirVar);
        }
        final String defaultConfigPath = arguments.optional("default-config-path");
        if (defaultConfigPath != null) {
            loader.defaultConfigPath(Path.of(defaultConfigPath));
        }
        return loader;
    }

    /**
     * {@code type}'s own field initialisers, nested the way {@link Schema#withDefaultsFromValue}
     * reads them. A fresh mapper rather than the one {@link TerraceLoader#load} binds with: this one
     * only converts an instance this process just constructed, never untrusted input.
     */
    private static Map<String, Object> defaultsOf(final ConfigurationType type) {
        return new ObjectMapper().convertValue(type.defaultInstance(), new TypeReference<Map<String, Object>>() {});
    }
}
