package de.timscho.config.spring;

import java.lang.reflect.Field;

import org.springframework.boot.autoconfigure.AutoConfiguration;
import org.springframework.boot.autoconfigure.condition.ConditionalOnProperty;
import org.springframework.boot.context.properties.EnableConfigurationProperties;
import org.springframework.context.annotation.Bean;
import org.springframework.core.env.Environment;

import de.timscho.config.core.descriptor.TypeDescriptor;
import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;

/**
 * Publishes a {@link Contract} bean for a service that only states two things through {@link
 * TerraceContractProperties}: its {@code @TerraceConfig}-annotated type and its environment
 * prefix — the zero-hand-written-{@code @Configuration}-class path {@link SpringContractProducer}
 * was documented as not providing on its own.
 *
 * <p>Registered through {@code META-INF/spring/
 * org.springframework.boot.autoconfigure.AutoConfiguration.imports}, the mechanism that replaced
 * {@code spring.factories} for {@code @Configuration} classes in Spring Boot 3 — unlike {@link
 * FileIndirectionEnvironmentPostProcessor}, which still uses {@code spring.factories} because
 * {@code EnvironmentPostProcessor} runs before any {@code @Configuration} class could.
 *
 * <p>Inactive by default: {@code @ConditionalOnProperty} gates the whole class on {@code
 * terrace.contract.type} being set, exactly as {@link TerraceContractProperties} documents. A
 * service with no such property gets no {@link Contract} bean and no reflective lookup is even
 * attempted.
 */
@AutoConfiguration
@EnableConfigurationProperties(TerraceContractProperties.class)
@ConditionalOnProperty(prefix = "terrace.contract", name = "type")
public class TerraceContractAutoConfiguration {

    /**
     * Looks up {@code <type>Descriptor.DESCRIPTOR} by reflection and assembles the {@link
     * Contract} through {@link SpringContractProducer}.
     *
     * <p>Only a top-level annotated type is supported here: a type nested inside another class
     * would need {@code terrace-config-processor}'s own {@code $}-joined naming, which this
     * class does not attempt to reconstruct from a plain class name. Declare the annotated type
     * at the top level, or call {@link SpringContractProducer#produce} directly.
     *
     * @throws IllegalStateException {@code terrace.contract.type} does not resolve to a class
     *                                with a generated {@code Descriptor} sibling carrying a
     *                                {@code public static final TypeDescriptor DESCRIPTOR} field
     */
    @Bean
    public Contract terraceContract(TerraceContractProperties properties, Environment environment) {
        TypeDescriptor descriptor = loadDescriptor(properties.getType());

        String appName = properties.getAppName() != null
                ? properties.getAppName()
                : environment.getProperty("spring.application.name", "application");
        App app = App.builder().name(appName).version(properties.getAppVersion()).build();

        String prefix = properties.getEnvPrefix();
        if (prefix == null || prefix.isEmpty()) {
            throw new IllegalStateException(
                    "terrace.contract.type is set to " + properties.getType()
                            + ", but terrace.contract.env-prefix is not; a contract needs an "
                            + "environment namespace to describe.");
        }

        return SpringContractProducer.produce(descriptor, prefix, app);
    }

    private static TypeDescriptor loadDescriptor(String typeName) {
        String descriptorName = typeName + "Descriptor";
        Class<?> descriptorClass;
        try {
            descriptorClass = Class.forName(descriptorName);
        } catch (ClassNotFoundException e) {
            throw new IllegalStateException(
                    "terrace.contract.type names " + typeName + ", but " + descriptorName
                            + " was not found on the classpath. Is the type annotated with "
                            + "@TerraceConfig, and did the annotation processor run?", e);
        }
        try {
            Field field = descriptorClass.getField("DESCRIPTOR");
            return (TypeDescriptor) field.get(null);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(
                    descriptorName + " carries no accessible `public static final TypeDescriptor "
                            + "DESCRIPTOR` field.", e);
        }
    }

}
