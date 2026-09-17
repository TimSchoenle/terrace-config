package de.timscho.config.example.service;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import de.timscho.config.core.contract.ContractAssembler;
import de.timscho.config.core.contract.ProducerIdentity;
import de.timscho.config.core.io.ContractCodec;
import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Producer;
import de.timscho.config.core.model.Schema;
import de.timscho.config.loader.TerraceLoader;
import java.util.Map;

/**
 * Renders this service's configuration as a contract document — {@code terrace-config-processor}'s
 * own rendering of exactly the {@link Config} class {@link OrdersService} loads, not a
 * hand-maintained description of it that could say something {@link Config} no longer does.
 *
 * <p>The Java counterpart of {@code examples/service/config.rs}'s {@code contract()}: same
 * loader, same prefix, same reserved key, differing only in {@link ProducerIdentity#forLoader}
 * naming {@code terrace-java} where the Rust crate names {@code figment}, and in how the default
 * value feeding {@link Schema#withDefaultsFromValue} is produced — the Rust side serialises
 * {@code Config::default()} through {@code figment}'s own {@code Serialize}; this side converts a
 * plain {@code new Config()} through the same {@link ObjectMapper} {@link TerraceLoader#load}
 * itself binds through, so the two can never disagree about what "no configuration at all" means.
 */
final class ContractGenerator {

    private ContractGenerator() {}

    /** Assembles and validates the {@link Contract} for {@link Config}, reachable under {@code ORDERS_}. */
    static Contract generate() {
        final Schema schema = TerraceLoader.of("ORDERS_")
                .reserve("ORDERS_PROFILE")
                .schema(ConfigDescriptor.DESCRIPTOR)
                .withDefaultsFromValue(defaultsAsMap());
        final App app = App.builder().name("orders-service").build();
        final Producer producer = ProducerIdentity.forLoader("terrace-java");
        return ContractAssembler.assemble(schema, app, producer);
    }

    /**
     * {@link Config}'s own field initialisers, nested the way {@link Schema#withDefaultsFromValue}
     * needs them — what a boot with no TOML file, no environment variable and no mounted secret at
     * all actually resolves to, the same "no configuration at all" state {@link
     * TerraceLoader#load} binds an empty merged map into. Not the mapper {@code load} uses to bind
     * a real configuration: this one only ever converts a value this same process just
     * constructed, never untrusted input, so sharing that instance would say more than is true.
     */
    private static Map<String, Object> defaultsAsMap() {
        return new ObjectMapper().convertValue(new Config(), new TypeReference<>() {});
    }

    /** {@link #generate()}, rendered as the same pretty-printed JSON {@link ContractCodec#write} writes to a file. */
    static String toJson(final Contract contract) {
        return new String(ContractCodec.write(contract), java.nio.charset.StandardCharsets.UTF_8);
    }
}
