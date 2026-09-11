package de.timscho.config.example.springservice;

import de.timscho.config.core.io.ContractCodec;
import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.spring.SpringContractProducer;

/**
 * Renders this service's configuration as a contract document through {@link
 * SpringContractProducer} — {@code terrace-config-processor}'s own rendering of exactly the
 * {@link OrdersProperties} class Spring binds, not a hand-maintained description of it.
 *
 * <p>The Spring counterpart of {@code terrace-config-example-service}'s own {@code
 * ContractGenerator}: same domain, same {@code ORDERS_} prefix, differing only in which producer
 * assembles the {@link Contract} — {@link SpringContractProducer} here, {@code TerraceLoader}
 * there.
 */
final class ContractGenerator {

    private ContractGenerator() {}

    /** Assembles and validates the {@link Contract} for {@link OrdersProperties}, reachable under {@code ORDERS_}. */
    static Contract generate() {
        App app = App.builder().name("orders-service").build();
        return SpringContractProducer.produce(OrdersPropertiesDescriptor.DESCRIPTOR, "ORDERS_", app);
    }

    /** {@link #generate()}, rendered as the same pretty-printed JSON {@link ContractCodec#write} writes to a file. */
    static String toJson(Contract contract) {
        return new String(ContractCodec.write(contract), java.nio.charset.StandardCharsets.UTF_8);
    }
}
