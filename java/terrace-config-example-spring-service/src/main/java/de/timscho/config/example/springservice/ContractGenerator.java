package de.timscho.config.example.springservice;

import java.util.Map;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;

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
 * there — and, since that producer never binds anything itself, in how the default value feeding
 * it is produced: a plain {@code new OrdersProperties()} converted through this class's own
 * {@link ObjectMapper}, mirroring what Spring's relaxed binding leaves a field at when nothing
 * supplies it.
 */
final class ContractGenerator {

    private ContractGenerator() {}

    /** Assembles and validates the {@link Contract} for {@link OrdersProperties}, reachable under {@code ORDERS_}. */
    static Contract generate() {
        App app = App.builder().name("orders-service").build();
        return SpringContractProducer.produce(OrdersPropertiesDescriptor.DESCRIPTOR, "ORDERS_", app, defaultsAsMap());
    }

    /**
     * {@link OrdersProperties}'s own field initialisers, nested the way {@link
     * SpringContractProducer#produce(de.timscho.config.core.descriptor.TypeDescriptor, String,
     * App, Map)}'s {@code defaults} parameter needs them. Not the mapper Spring's own {@code
     * Binder} binds a real configuration through: this one only ever converts a value this same
     * process just constructed, never untrusted input.
     */
    private static Map<String, Object> defaultsAsMap() {
        return new ObjectMapper().convertValue(new OrdersProperties(), new TypeReference<Map<String, Object>>() {});
    }

    /** {@link #generate()}, rendered as the same pretty-printed JSON {@link ContractCodec#write} writes to a file. */
    static String toJson(Contract contract) {
        return new String(ContractCodec.write(contract), java.nio.charset.StandardCharsets.UTF_8);
    }
}
