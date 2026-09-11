package de.timscho.config.example.springservice;

import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;
import org.springframework.boot.context.properties.EnableConfigurationProperties;
import org.springframework.context.ConfigurableApplicationContext;

/**
 * A small HTTP-shaped service, wired to load its configuration through Spring Boot and {@code
 * terrace-config-spring-boot}'s {@code _FILE} indirection — the Spring counterpart of {@code
 * terrace-config-example-service}'s {@code OrdersService} and {@code
 * rust/examples/service/main.rs}.
 *
 * <pre>{@code
 * ./gradlew :terrace-config-example-spring-service:run
 * ORDERS_PORT=9090 ./gradlew :terrace-config-example-spring-service:run
 * ORDERS_LOG_LEVEL=debug ./gradlew :terrace-config-example-spring-service:run
 * ./gradlew :terrace-config-example-spring-service:run --args=--contract
 * }</pre>
 *
 * <p>A real deployment never sets {@code ORDERS_DATABASE_URL} in plain environment text — it
 * mounts a {@code Secret} volume and points {@code ORDERS_DATABASE_URL_FILE} at one file.
 * {@code OrdersServiceFileIndirectionIntegrationTest} boots this exact application that way,
 * without touching the real process environment.
 *
 * <p>{@code spring-boot-starter} only, not {@code -web}: this example is about configuration, not
 * about serving anything, so {@link SpringApplication} auto-detects a non-web application and
 * opens no port of its own.
 *
 * <p>{@code --contract} is a second, optional job the same binary can do beside its normal boot
 * path — see {@link ContractGenerator}. {@code contract.json} in this module's own directory is
 * that rendering, checked fresh by {@code ContractTest} rather than trusted merely because it
 * once matched.
 */
@SpringBootApplication
@EnableConfigurationProperties(OrdersProperties.class)
public class OrdersServiceApplication {

    static void main(String[] args) {
        if (args.length > 0 && "--contract".equals(args[0])) {
            System.out.println(ContractGenerator.toJson(ContractGenerator.generate()));
            return;
        }

        try (ConfigurableApplicationContext context = SpringApplication.run(OrdersServiceApplication.class, args)) {
            report(context.getBean(OrdersProperties.class));
        }
    }

    static void report(OrdersProperties properties) {
        System.out.println("listening on " + properties.getBindAddr() + ":" + properties.getPort());
        System.out.println("log level: " + properties.getLogLevel());
        System.out.println("database pool: " + properties.getDatabase().getMaxConnections() + " connections");

        // A real service passes this straight to its connection pool and never prints it.
        // Printed here only to make the point in the terminal: the value loaded, and it is not
        // the compiled default once an operator overrides it.
        String url = properties.getDatabase().getUrl();
        boolean isDefault = url.contains("orders_dev");
        System.out.println("database: " + (isDefault ? "local development default" : "overridden"));
    }
}
