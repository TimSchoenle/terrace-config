package de.timscho.config.example.service;

import de.timscho.config.core.contract.ContractAssembler;
import de.timscho.config.core.contract.ProducerIdentity;
import de.timscho.config.core.io.ContractCodec;
import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Producer;
import de.timscho.config.core.model.Schema;
import de.timscho.config.loader.TerraceLoader;

/**
 * Renders this service's configuration as a contract document — {@code terrace-config-processor}'s
 * own rendering of exactly the {@link Config} class {@link OrdersService} loads, not a
 * hand-maintained description of it that could say something {@link Config} no longer does.
 *
 * <p>The Java counterpart of {@code examples/service/config.rs}'s {@code contract()}: same
 * loader, same prefix, same reserved key, differing only in {@link ProducerIdentity#forLoader}
 * naming {@code terrace-java} where the Rust crate names {@code figment}.
 */
final class ContractGenerator {

    private ContractGenerator() {}

    /** Assembles and validates the {@link Contract} for {@link Config}, reachable under {@code ORDERS_}. */
    static Contract generate() {
        Schema schema = TerraceLoader.of("ORDERS_").reserve("ORDERS_PROFILE").schema(ConfigDescriptor.DESCRIPTOR);
        App app = App.builder().name("orders-service").build();
        Producer producer = ProducerIdentity.forLoader("terrace-java");
        return ContractAssembler.assemble(schema, app, producer);
    }

    /** {@link #generate()}, rendered as the same pretty-printed JSON {@link ContractCodec#write} writes to a file. */
    static String toJson(Contract contract) {
        return new String(ContractCodec.write(contract), java.nio.charset.StandardCharsets.UTF_8);
    }
}
