package de.timscho.config.spring;

import org.springframework.boot.context.properties.ConfigurationProperties;

import lombok.Getter;
import lombok.Setter;

/**
 * The handful of things only a service itself knows, and {@link TerraceContractAutoConfiguration}
 * cannot invent: which {@code @TerraceConfig}-annotated type describes its configuration, and the
 * environment prefix it is reachable under.
 *
 * <pre>{@code
 * terrace.contract.type=com.example.AppConfig
 * terrace.contract.env-prefix=MYAPP_
 * terrace.contract.app-name=my-service
 * }</pre>
 *
 * <p>Absent {@link #type}, the auto-configuration publishes no {@link
 * de.timscho.config.core.model.Contract} bean at all — see {@link
 * TerraceContractAutoConfiguration}'s own {@code @ConditionalOnProperty}. This is the opt-in
 * {@link SpringContractProducer} was documented as deliberately not providing on its own: a
 * service still states its own type and prefix, only no longer writes the {@code
 * @Configuration} class that calls {@link SpringContractProducer#produce} by hand.
 */
@Getter
@Setter
@ConfigurationProperties(prefix = "terrace.contract")
public class TerraceContractProperties {

    /**
     * The fully qualified name of the {@code @TerraceConfig}-annotated configuration type. Its
     * generated {@code <Type>Descriptor.DESCRIPTOR} is what {@link TerraceContractAutoConfiguration}
     * looks up by reflection -- required for the auto-configuration to activate at all.
     */
    private String type;

    /** The environment namespace this service's configuration lives under, e.g. {@code MYAPP_}. */
    private String envPrefix;

    /** {@link de.timscho.config.core.model.App#getName()}. Defaults to {@code spring.application.name}. */
    private String appName;

    /** {@link de.timscho.config.core.model.App#getVersion()}. Omitted when unset. */
    private String appVersion;
}
