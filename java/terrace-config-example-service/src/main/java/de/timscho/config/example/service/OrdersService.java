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
 * ./gradlew :terrace-config-example-service:run --args=--contract
 * }</pre>
 *
 * <p>A real deployment never sets {@code ORDERS_DATABASE__URL} in plain environment text — it
 * mounts a {@code Secret} volume and sets {@code ORDERS_SECRETS_DIR} at that path, or points
 * {@code ORDERS_DATABASE__URL_FILE} at one file. {@code OrdersServiceConfigTest} boots this exact
 * configuration every way {@link TerraceLoader} supports.
 *
 * <p>{@code --contract} is a second, optional job the same binary can do beside its normal boot
 * path — see {@link ContractGenerator}. {@code contract.json} in this module's own directory is
 * that rendering, checked fresh by {@code ContractTest} rather than trusted merely because it
 * once matched.
 */
public final class OrdersService {

    private OrdersService() {}

    public static void main(String[] args) {
        if (args.length > 0 && "--contract".equals(args[0])) {
            System.out.println(ContractGenerator.toJson(ContractGenerator.generate()));
            return;
        }

        Config config = TerraceLoader.of("ORDERS_").reserve("ORDERS_PROFILE").load(Config.class);

        System.out.println("listening on " + config.getBindAddr() + ":" + config.getPort());
        System.out.println("log level: " + config.getLogLevel());
        System.out.println("database pool: " + config.getDatabase().getMaxConnections() + " connections");

        // A real service passes this straight to its connection pool and never prints it.
        // Printed here only to make the point in the terminal: the value loaded, and it is not
        // the compiled default once an operator overrides it.
        String url = config.getDatabase().getUrl();
        boolean isDefault = url.contains("orders_dev");
        System.out.println("database: " + (isDefault ? "local development default" : "overridden"));
    }
}
