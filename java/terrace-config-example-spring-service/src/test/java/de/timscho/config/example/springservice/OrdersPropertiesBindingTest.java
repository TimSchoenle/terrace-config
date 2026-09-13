package de.timscho.config.example.springservice;

import static org.assertj.core.api.Assertions.assertThat;

import org.junit.jupiter.api.Test;
import org.springframework.boot.context.properties.EnableConfigurationProperties;
import org.springframework.boot.test.context.runner.ApplicationContextRunner;

/**
 * The example's own {@link OrdersProperties}, bound through a real Spring {@code Binder} exactly
 * as {@link OrdersServiceApplication} boots it — the property-binding half of the story {@code
 * OrdersServiceFileIndirectionIntegrationTest} finishes with the one layer binding alone cannot
 * exercise. Mirrors the scenarios {@code tests/example_service.rs} and {@code
 * terrace-config-example-service}'s own {@code OrdersServiceConfigTest} pin for the other two
 * producers.
 */
class OrdersPropertiesBindingTest {

    private final ApplicationContextRunner contextRunner =
            new ApplicationContextRunner().withUserConfiguration(TestConfig.class);

    @EnableConfigurationProperties(OrdersProperties.class)
    static class TestConfig {}

    @Test
    void compiledDefaultsAreEnoughToBoot() {
        contextRunner.run(context -> {
            OrdersProperties properties = context.getBean(OrdersProperties.class);

            assertThat(properties.getBindAddr()).isEqualTo("127.0.0.1");
            assertThat(properties.getPort()).isEqualTo(8080);
            assertThat(properties.getLogLevel()).isEqualTo(LogLevel.INFO);
            assertThat(properties.getDatabase().getMaxConnections()).isEqualTo(10);
            assertThat(properties.getDatabase().getUrl()).isEqualTo("postgres://localhost/orders_dev");
        });
    }

    @Test
    void propertyValuesOverrideTheListenerAndTheDatabase() {
        contextRunner
                .withPropertyValues(
                        "orders.bind-addr=0.0.0.0",
                        "orders.port=9090",
                        "orders.database.url=postgres://file/orders",
                        "orders.database.max-connections=25")
                .run(context -> {
                    OrdersProperties properties = context.getBean(OrdersProperties.class);

                    assertThat(properties.getBindAddr()).isEqualTo("0.0.0.0");
                    assertThat(properties.getPort()).isEqualTo(9090);
                    assertThat(properties.getDatabase().getMaxConnections()).isEqualTo(25);
                    assertThat(properties.getDatabase().getUrl()).isEqualTo("postgres://file/orders");
                });
    }

    @Test
    void logLevelBindsCaseInsensitivelyUnlikeTheVanillaLoader() {
        contextRunner
                .withPropertyValues("orders.log-level=Debug")
                .run(context -> assertThat(
                                context.getBean(OrdersProperties.class).getLogLevel())
                        .isEqualTo(LogLevel.DEBUG));
    }

    @Test
    void anUnrecognisedLogLevelFailsTheContextInsteadOfBootingOnADefault() {
        contextRunner
                .withPropertyValues("orders.log-level=verbose")
                .run(context -> assertThat(context).hasFailed());
    }
}
