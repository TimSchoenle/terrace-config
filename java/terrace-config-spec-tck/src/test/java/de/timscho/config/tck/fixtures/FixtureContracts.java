package de.timscho.config.tck.fixtures;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import de.timscho.config.core.contract.ContractAssembler;
import de.timscho.config.core.contract.ProducerIdentity;
import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.External;
import de.timscho.config.core.model.ExternalUnknownPolicy;
import de.timscho.config.core.model.ExternalVar;
import de.timscho.config.core.model.Producer;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.TextForm;
import de.timscho.config.core.schema.Refine;
import de.timscho.config.core.schema.Refinement;
import de.timscho.config.loader.TerraceLoader;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/**
 * Assembles the four named spec cases ({@code minimal}, {@code full-surface},
 * {@code unnameable-key}, {@code required-entries}) as real {@link Contract}s, each from its own fixture type in this
 * package, run through the real {@code terrace-config-loader} and the real Jackson 2 codec —
 * exactly the pipeline a consuming service runs, not a hand-maintained restatement of what one
 * would render. Mirrors {@code terrace-config-example-service}'s {@code ContractGenerator},
 * generalised from one service to the three spec cases {@code JavaConformanceTest} checks.
 */
public final class FixtureContracts {

    private static final ObjectMapper MAPPER = new ObjectMapper();

    private FixtureContracts() {}

    /** The {@code minimal} case, reachable under {@code MINIMAL_}. */
    public static Contract minimal() {
        final Schema schema = TerraceLoader.of("MINIMAL_")
                .schema(MinimalConfigDescriptor.DESCRIPTOR)
                .withDefaultsFromValue(defaultsAsMap(new MinimalConfig()));
        final App app = App.builder().name("minimal").version("1.0.0").build();
        return ContractAssembler.assemble(schema, app, producer());
    }

    /** The {@code unnameable-key} case, reachable under {@code EDGE_}. */
    public static Contract unnameableKey() {
        final Schema schema = TerraceLoader.of("EDGE_")
                .schema(UnnameableConfigDescriptor.DESCRIPTOR)
                .withDefaultsFromValue(defaultsAsMap(new UnnameableConfig()));
        final App app = App.builder().name("edge").version("1.0.0").build();
        return ContractAssembler.assemble(schema, app, producer());
    }

    /** The {@code full-surface} case, reachable under {@code PORTFOLIO_}. */
    public static Contract fullSurface() {
        final Schema schema = TerraceLoader.of("PORTFOLIO_")
                .reserve("PORTFOLIO_PROFILE")
                .schema(FullSurfaceConfigDescriptor.DESCRIPTOR)
                .withDefaultsFromValue(defaultsAsMap(new FullSurfaceConfig()));
        final App app = App.builder().name("portfolio").version("2.5.0").build();
        return ContractAssembler.assemble(schema, app, producer(), fullSurfaceExternal());
    }

    /**
     * The {@code required-entries} case, reachable under {@code SITE_}: the legal-pages library's
     * two refinements applied relative to where this host mounts it, after the defaults were
     * observed — so {@code legal.documents}' empty default becomes a requirement and {@code
     * legal.links}' default, which already holds {@code home}, survives.
     */
    public static Contract requiredEntries() {
        final Schema schema = TerraceLoader.of("SITE_")
                .schema(RequiredEntriesConfigDescriptor.DESCRIPTOR)
                .withDefaultsFromValue(defaultsAsMap(new RequiredEntriesConfig()))
                .refineWith("legal", legalPagesRefinements());
        final App app = App.builder().name("site").version("1.0.0").build();
        return ContractAssembler.assemble(schema, app, producer());
    }

    /** What the legal-pages library publishes, relative to its mount. */
    private static Refine legalPagesRefinements() {
        return () -> List.of(
                new Refine.At("documents", Refinement.requiredEntries(List.of("imprint", "privacy"))),
                new Refine.At("links", Refinement.requiredEntries(List.of("home"))));
    }

    private static Producer producer() {
        return ProducerIdentity.forLoader("terrace-java");
    }

    /**
     * What {@code full-surface} declares outside its own prefix: a runtime-owned port, a JVM
     * option string nobody's configuration key claims, {@code KUBERNETES_*}/{@code HOSTNAME}
     * ignored, and everything else refused — the Java-native counterpart of the same case's
     * {@code PORT}/{@code RUST_LOG} pairing in the Rust reference corpus, naming variables this
     * runtime actually reads rather than another language's toolchain's.
     */
    private static External fullSurfaceExternal() {
        final Map<String, Object> portConstraint = new TreeMap<>();
        portConstraint.put("type", "integer");
        portConstraint.put("minimum", 0);
        portConstraint.put("maximum", 65535);
        final Map<String, Object> portTextConstraint = new TreeMap<>();
        portTextConstraint.put("type", "string");
        portTextConstraint.put("pattern", "^\\s*\\+?[0-9]+\\s*$");

        final ExternalVar port = ExternalVar.builder()
                .name("PORT")
                .owner("runtime")
                .docs("Bind port. Read by the server toolchain, not by this loader.")
                .ty("int")
                .constraint(portConstraint)
                .textConstraint(portTextConstraint)
                .textForm(TextForm.INTEGER)
                .defaultText("8080")
                .required(false)
                .secret(false)
                .build();
        final ExternalVar javaToolOptions = ExternalVar.builder()
                .name("JAVA_TOOL_OPTIONS")
                .owner("jvm")
                .docs("Extra JVM flags, read by the launcher before this process's main starts.")
                .ty("String")
                .textForm(TextForm.TEXT)
                .required(false)
                .secret(false)
                .build();
        return External.builder()
                .env(List.of(port, javaToolOptions))
                .ignore(List.of("KUBERNETES_*", "HOSTNAME"))
                .unknown(ExternalUnknownPolicy.REJECT)
                .build();
    }

    /**
     * {@code instance}'s own field initialisers, nested the way {@link Schema#withDefaultsFromValue}
     * needs them — what a boot with no TOML file, no environment variable and no mounted secret at
     * all actually resolves to. Not the mapper the loader itself binds through: this one only ever
     * converts a value this same process just constructed, never untrusted input.
     */
    private static Map<String, Object> defaultsAsMap(final Object instance) {
        return MAPPER.convertValue(instance, new TypeReference<Map<String, Object>>() {});
    }
}
