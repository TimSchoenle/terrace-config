package de.timscho.config.example.service;

import de.timscho.config.loader.TerraceLoader;

/**
 * A small HTTP-shaped service, wired to load its configuration through {@link TerraceLoader} —
 * the Java mirror of {@code rust/examples/service/main.rs}.
 *
 * <pre>{@code
 * ./gradlew :terrace-config-example-service:run
 * ORDERS_PORT=9090 ./gradlew :terrace-config-example-service:run
 * ORDERS_LOG_LEVEL=debug ./gradlew :terrace-config-example-service:run
 * }</pre>
 *
 * <p>A real deployment never sets {@code ORDERS_DATABASE__URL} in plain environment text — it
 * mounts a {@code Secret} volume and sets {@code ORDERS_SECRETS_DIR} at that path, or points
 * {@code ORDERS_DATABASE__URL_FILE} at one file. {@code OrdersServiceConfigTest} boots this exact
 * configuration every way {@link TerraceLoader} supports.
 *
 * <p>The contract is the build's, not this binary's: the {@code de.timscho.terrace-config} plugin
 * emits it from {@link Config} without starting the service ({@code ./gradlew
 * :terrace-config-example-service:terraceContract}), and {@code terraceContractCheck}, part of
 * {@code check}, fails when {@code contract.json} in this module's directory no longer matches.
 */
public final class OrdersService {

    private OrdersService() {}

    // BanSystemOut assumes System.out is always a substitute for a logger; here it is this demo
    // binary's actual terminal output for a human running `./gradlew ... :run` by hand (the config
    // values it booted with) -- there is no SLF4J-worthy log event to attribute it to, and no other
    // consumer for it to reach.
    @SuppressWarnings("checkstyle:BanSystemOut")
    public static void main(final String[] args) {
        final Config config =
                TerraceLoader.of("ORDERS_").reserve("ORDERS_PROFILE").load(Config.class);

        System.out.println("listening on " + config.getBindAddr() + ":" + config.getPort());
        System.out.println("log level: " + config.getLogLevel());
        System.out.println("database pool: " + config.getDatabase().getMaxConnections() + " connections");

        // A real service passes this straight to its connection pool and never prints it.
        // Printed here only to make the point in the terminal: the value loaded, and it is not
        // the compiled default once an operator overrides it.
        final String url = config.getDatabase().getUrl();
        final boolean isDefault = url.contains("orders_dev");
        System.out.println("database: " + (isDefault ? "local development default" : "overridden"));
    }
}
