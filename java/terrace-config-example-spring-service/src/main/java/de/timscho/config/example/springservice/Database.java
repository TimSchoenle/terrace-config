package de.timscho.config.example.springservice;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.Getter;
import lombok.Setter;

import de.timscho.config.annotations.Secret;
import de.timscho.config.annotations.TerraceConfig;

/** The database this service persists orders to. */
@TerraceConfig
@Getter
@Setter
public final class Database {

    /**
     * Connection string. A real deployment supplies this through file indirection, never through
     * a plain environment variable, so the compiled default below is the only value this field
     * is meant to ever hold in plain sight.
     */
    // Plain prose above, no `{@code}` markup: `Javadocs.normalize` carries a field's doc comment
    // into `contract.json` verbatim rather than rendering javadoc tags.
    //
    // `access = WRITE_ONLY`: Spring's own `Binder` ignores Jackson annotations entirely, so this
    // changes nothing about how a real deployment binds `url` — it only tells `ContractGenerator`'s
    // own `ObjectMapper().convertValue(new OrdersProperties(), Map.class)` (used to find each
    // key's observed default for `Schema.withDefaultsFromValue`) to skip this field, exactly as
    // the Rust example's `SecretString` field's `#[serde(skip_serializing)]` does. Without it, the
    // very real default below would flow through as an *observed* default, and
    // `ContractValidator`'s own "a secret must not carry a default" refusal
    // (`SecretWithDefaultException`) would then refuse the contract this class renders.
    @JsonProperty(access = JsonProperty.Access.WRITE_ONLY)
    @Secret
    private String url = "postgres://localhost/orders_dev";

    /** Maximum number of pooled connections. */
    private int maxConnections = 10;
}
