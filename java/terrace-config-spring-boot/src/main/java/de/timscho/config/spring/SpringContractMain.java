package de.timscho.config.spring;

import de.timscho.config.core.contract.ConfigurationType;
import de.timscho.config.core.contract.ContractAssembler;
import de.timscho.config.core.contract.ProducerArguments;
import de.timscho.config.core.contract.ProducerCommandLine;
import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import java.util.List;
import java.util.Set;

/**
 * Emits the contract of a Spring Boot service at build time, without starting it: the entry point
 * the {@code de.timscho.terrace-config} Gradle plugin's {@code terraceContract} task runs on the
 * service's own runtime classpath.
 *
 * <pre>{@code
 * java -cp <runtime classpath> de.timscho.config.spring.SpringContractMain \
 *     --type com.example.AppProperties --prefix MYAPP_ --app-name my-service \
 *     --out build/terrace/contract.json
 * }</pre>
 *
 * <p>Defaults come from the properties class itself: a default-constructed instance, which is what
 * Spring's binder leaves a field at when nothing supplies it, converted to the nested map that
 * {@link SpringContractProducer}'s {@code defaults} parameter reads. No {@code ApplicationContext}
 * is created, so nothing the service would do at boot (connecting, reading secrets, failing on a
 * missing value) happens while its build emits the contract. Build identity is {@code
 * terrace-contract stamp}'s, applied to the emitted document.
 *
 * <p><b>Needs {@code terrace-config-core-jackson2} on the classpath it runs with.</b> This module
 * takes no Jackson dependency of its own (see its {@code build.gradle.kts}), so a Spring Boot 4
 * service is never handed a second Jackson major at runtime; the codec is a build-time
 * dependency, which the Gradle plugin's {@code terraceContractGenerator} configuration supplies.
 *
 * <p>Exits {@code 0} when the document was written, {@code 1} when it was refused, {@code 2} on a
 * usage or classpath error; see {@link ProducerCommandLine}.
 */
public final class SpringContractMain {

    /** What {@code --help} prints. */
    static final String USAGE = """
            Usage: de.timscho.config.spring.SpringContractMain
                       --type <class> --prefix <PREFIX_> --app-name <name> [options]

            Emits the contract of a @TerraceConfig @ConfigurationProperties type bound by Spring Boot.

              --type <class>             the root @TerraceConfig type, by binary name
                                         (com.example.AppProperties, com.example.Outer$Inner)
              --prefix <PREFIX_>         the environment prefix, taken verbatim
              --app-name <name>          the contract's app.name
              --nesting-separator <sep>  the nesting separator (default _, Spring's own)
              --file-suffix <suffix>     the indirection suffix (default _FILE)
              --out <file>               write the document here instead of standard output

            Needs terrace-config-core-jackson2 on the classpath.
            Exit status: 0 written, 1 refused, 2 usage or classpath error.
            """;

    /** What {@link GeneratorCodec} links against, checked by name before it is first touched. */
    private static final List<String> CODEC_CLASSES =
            List.of("de.timscho.config.core.io.ContractCodec", "com.fasterxml.jackson.databind.ObjectMapper");

    private SpringContractMain() {}

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
                Set.of("type", "prefix", "app-name", "nesting-separator", "file-suffix"),
                Set.of(),
                SpringContractMain::produce,
                GeneratorCodec::write);
    }

    static Contract produce(final ProducerArguments arguments) {
        requireCodec();
        final ConfigurationType type =
                ConfigurationType.named(arguments.required("type"), SpringContractMain.class.getClassLoader());
        SpringDialect dialect = SpringDialect.standard();
        final String separator = arguments.optional("nesting-separator");
        if (separator != null) {
            dialect = dialect.withNestingSeparator(separator);
        }
        final String suffix = arguments.optional("file-suffix");
        if (suffix != null) {
            dialect = dialect.withIndirectionSuffix(suffix);
        }
        final App app = App.builder().name(arguments.required("app-name")).build();
        return SpringContractProducer.produce(
                type.descriptor(),
                arguments.required("prefix"),
                dialect,
                app,
                ContractAssembler.noExternalSurface(),
                GeneratorCodec.defaultsOf(type.defaultInstance()));
    }

    /**
     * Fails with a usage error naming the missing module, rather than a {@link NoClassDefFoundError}
     * halfway through, when the codec this class compiles against is not on the classpath.
     */
    private static void requireCodec() {
        for (final String name : CODEC_CLASSES) {
            try {
                Class.forName(name, false, SpringContractMain.class.getClassLoader());
            } catch (ClassNotFoundException e) {
                throw new IllegalArgumentException(
                        name + " is not on the classpath: SpringContractMain writes the contract with"
                                + " terrace-config-core-jackson2. Add it to the classpath the generator runs"
                                + " with (the Gradle plugin's `terraceContractGenerator` configuration).",
                        e);
            }
        }
    }
}
