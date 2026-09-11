package de.timscho.config.example.service;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.Getter;
import lombok.Setter;

import de.timscho.config.annotations.Nested;
import de.timscho.config.annotations.TerraceConfig;
import de.timscho.config.annotations.Values;
import de.timscho.config.loader.TerraceLoader;

/**
 * The root configuration a small "orders" service reads at boot, through {@link TerraceLoader}.
 *
 * <p>Mirrors {@code rust/examples/service/config.rs} field for field, under the same {@code
 * ORDERS_} prefix: the two examples tell one story about what this contract looks like in each
 * language, and {@code OrdersServiceConfigTest} pins the same scenarios that
 * {@code tests/example_service.rs} does.
 *
 * <p>A mutable class with field initialisers, not a record. {@link TerraceLoader#load} binds
 * through Jackson's default no-args-constructor-then-setters path: a key the merged configuration
 * does not supply never has its setter called, so the field keeps the default written here — the
 * Java equivalent of a Rust {@code #[serde(default = "…")]}. A record instead demands a value for
 * every constructor argument, which is the wrong shape for a type meant to boot on nothing at
 * all.
 *
 * <p>Every environment variable and secrets-directory key this loader reads is folded to lower
 * case ({@link de.timscho.config.loader.Dialect#keyPath}), and a TOML file is written the same
 * way by convention, so a multi-word field needs {@link JsonProperty} to spell the snake_case key
 * every layer actually uses — {@code port} does not, being one word already.
 *
 * <p>Also {@code @TerraceConfig}: {@code terrace-config-processor} reads exactly the fields and
 * {@code @JsonProperty} renames above to generate {@code ConfigDescriptor}, which {@code
 * ContractGenerator} turns into {@code contract.json} — one set of annotations, read by two
 * independent things (Jackson's binder and the processor), which is what keeps the two from
 * describing different keys. See {@code FieldResolver.resolve} in {@code terrace-config-processor}
 * for the fix that made this true.
 */
@TerraceConfig
@Getter
@Setter
public class Config {

    /** Address the HTTP listener binds to. */
    @JsonProperty("bind_addr")
    private String bindAddr = "127.0.0.1";

    /** Port the HTTP listener binds to. */
    private int port = 8080;

    /** Where orders are persisted. */
    @Nested
    private Database database = new Database();

    /** How much the service says. */
    @JsonProperty("log_level")
    @Values
    private LogLevel logLevel = LogLevel.INFO;
}
