package de.timscho.config.example.springservice;

import lombok.Getter;
import lombok.Setter;
import org.springframework.boot.context.properties.ConfigurationProperties;

import de.timscho.config.annotations.Nested;
import de.timscho.config.annotations.TerraceConfig;
import de.timscho.config.annotations.Values;

/**
 * The root configuration a small "orders" service reads at boot, bound by Spring's own {@code
 * Binder} rather than {@code terrace-config-loader} — the same service as {@code
 * terrace-config-example-service}, field for field, so the two can be read side by side as one
 * story about what this contract looks like through each producer.
 *
 * <p>A mutable class with field initialisers: Spring's relaxed binding, like Jackson's default
 * bean binding, only calls a setter for a property actually present in the environment, so a key
 * nothing supplies simply keeps the default written here.
 *
 * <p>No {@code @JsonProperty}-equivalent is needed for {@link #bindAddr} the way {@code
 * terrace-config-example-service}'s {@code Config} needs one: Spring's relaxed binding already
 * matches {@code ORDERS_BIND_ADDR}, {@code orders.bind-addr} and {@code orders.bindAddr} to this
 * same camelCase field. What Spring's binder does <b>not</b> give a service is {@code _FILE}
 * indirection — {@code terrace-config-spring-boot}'s {@link
 * de.timscho.config.spring.FileIndirectionEnvironmentPostProcessor} adds exactly that, with its
 * own documented divergence: see {@link Database#maxConnections} and {@code
 * OrdersServiceFileIndirectionIntegrationTest}.
 *
 * <p>Also {@code @TerraceConfig}: {@code ContractGenerator} builds a real {@link
 * de.timscho.config.core.model.Contract} from exactly this class through {@code
 * SpringContractProducer}, the same generated-and-CI-checked {@code contract.json} story {@code
 * terrace-config-example-service} tells for the vanilla loader.
 */
@TerraceConfig
@Getter
@Setter
@ConfigurationProperties(prefix = "orders")
public class OrdersProperties {

    /** Address the HTTP listener binds to. */
    private String bindAddr = "127.0.0.1";

    /** Port the HTTP listener binds to. */
    private int port = 8080;

    /** Where orders are persisted. */
    @Nested
    private Database database = new Database();

    /** How much the service says. */
    @Values
    private LogLevel logLevel = LogLevel.INFO;
}
