package de.timscho.config.example.springservice;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import org.springframework.boot.builder.SpringApplicationBuilder;
import org.springframework.context.ConfigurableApplicationContext;
import org.springframework.core.env.MapPropertySource;
import org.springframework.core.env.StandardEnvironment;
import org.springframework.core.env.SystemEnvironmentPropertySource;

/**
 * {@link OrdersServiceApplication} booted for real — the one thing {@link
 * OrdersPropertiesBindingTest} cannot exercise, because {@code _FILE} indirection is an {@link
 * org.springframework.boot.env.EnvironmentPostProcessor}, and {@code ApplicationContextRunner}
 * never runs one: it builds a plain application context directly, the same shortcut {@code
 * FileIndirectionEnvironmentPostProcessorTest} takes from the other side, unit-testing the
 * post-processor itself against a bare {@code MockEnvironment} rather than a booted application.
 * This class is the seam between the two — a real {@link
 * org.springframework.boot.SpringApplication#run} that proves the post-processor, {@code
 * terrace-config-spring-boot}'s {@code spring.factories} registration, and {@link
 * OrdersProperties}'s binding all actually meet.
 *
 * <p>Never touches this JVM's real environment: {@link #fakeSystemEnvironment} replaces the
 * {@link StandardEnvironment#SYSTEM_ENVIRONMENT_PROPERTY_SOURCE_NAME} property source with a
 * {@link SystemEnvironmentPropertySource} of its own <i>before</i> the application boots, so the
 * post-processor — which reads exactly that named source — sees only what a test arranged, the
 * same hermeticity {@code tests/example_service.rs}'s {@code Harness} and {@code
 * terrace-config-example-service}'s own test give the other two producers.
 */
class OrdersServiceFileIndirectionIntegrationTest {

    @TempDir
    Path tempDir;

    @Test
    void theDatabaseUrlReachesTheBoundBeanThroughFileIndirection() throws IOException {
        Path secret = tempDir.resolve("db-url.txt");
        Files.writeString(secret, "postgres://indirect/orders");

        try (ConfigurableApplicationContext context = boot(Map.of("ORDERS_DATABASE_URL_FILE", secret.toString()))) {
            OrdersProperties properties = context.getBean(OrdersProperties.class);
            assertThat(properties.getDatabase().getUrl()).isEqualTo("postgres://indirect/orders");
        }
    }

    /**
     * {@link de.timscho.config.spring.SpringDialect#propertyName} folds every underscore to a dot,
     * with no way to tell "the next nesting level" from "a word boundary inside one field's own
     * name" — the divergence its own class documentation calls out. {@code
     * ORDERS_DATABASE_MAX_CONNECTIONS_FILE} resolves to {@code orders.database.max.connections}:
     * four segments, one level deeper than {@link Database#maxConnections} actually sits at. Spring's
     * binder finds no such nested path and silently binds nothing, so the compiled default survives
     * rather than a corrupted value reaching the bean.
     */
    @Test
    void aMultiWordPropertyCannotBeReachedThroughFileIndirection() throws IOException {
        Path secret = tempDir.resolve("max-connections.txt");
        Files.writeString(secret, "999");

        try (ConfigurableApplicationContext context =
                boot(Map.of("ORDERS_DATABASE_MAX_CONNECTIONS_FILE", secret.toString()))) {
            OrdersProperties properties = context.getBean(OrdersProperties.class);
            assertThat(properties.getDatabase().getMaxConnections()).isEqualTo(10);
        }
    }

    @Test
    void theFileBackedUrlOutranksAnOrdinaryPropertyAlreadyPresent() throws IOException {
        Path secret = tempDir.resolve("db-url.txt");
        Files.writeString(secret, "postgres://indirect/orders");

        StandardEnvironment environment = fakeSystemEnvironment(Map.of("ORDERS_DATABASE_URL_FILE", secret.toString()));
        // A source added the way `application.properties` would be — ordinary precedence, well
        // below the front-of-line slot `FileIndirectionEnvironmentPostProcessor` claims for the
        // property it resolves.
        environment
                .getPropertySources()
                .addLast(new MapPropertySource(
                        "applicationConfig", Map.of("orders.database.url", "postgres://ordinary-property/orders")));

        try (ConfigurableApplicationContext context = new SpringApplicationBuilder(OrdersServiceApplication.class)
                .environment(environment)
                .run()) {
            OrdersProperties properties = context.getBean(OrdersProperties.class);
            assertThat(properties.getDatabase().getUrl()).isEqualTo("postgres://indirect/orders");
        }
    }

    private static ConfigurableApplicationContext boot(Map<String, String> fakeEnvironment) {
        return new SpringApplicationBuilder(OrdersServiceApplication.class)
                .environment(fakeSystemEnvironment(fakeEnvironment))
                .run();
    }

    /** A real {@link StandardEnvironment} whose system-environment source carries {@code entries} instead of this process's own. */
    private static StandardEnvironment fakeSystemEnvironment(Map<String, String> entries) {
        StandardEnvironment environment = new StandardEnvironment();
        environment
                .getPropertySources()
                .replace(
                        StandardEnvironment.SYSTEM_ENVIRONMENT_PROPERTY_SOURCE_NAME,
                        new SystemEnvironmentPropertySource(
                                StandardEnvironment.SYSTEM_ENVIRONMENT_PROPERTY_SOURCE_NAME,
                                new LinkedHashMap<String, Object>(entries)));
        return environment;
    }
}
