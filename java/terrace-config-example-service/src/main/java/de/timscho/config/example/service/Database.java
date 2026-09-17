package de.timscho.config.example.service;

import com.fasterxml.jackson.annotation.JsonProperty;
import de.timscho.config.annotations.Secret;
import de.timscho.config.annotations.TerraceConfig;
import lombok.Getter;
import lombok.Setter;

/** The database this service persists orders to. */
@TerraceConfig
@Getter
@Setter
public final class Database {

    /**
     * Connection string. A real deployment supplies this through the secrets directory or file
     * indirection, never through a plain environment variable, so the compiled default below is
     * the only value this field is meant to ever hold in plain sight.
     */
    // Plain prose above, no `{@code}` markup: `Javadocs.normalize` carries a field's doc comment
    // into `contract.json` verbatim rather than rendering javadoc tags, so a `{@code _FILE}` here
    // would reach that document as the literal seven characters `{@code`.
    //
    // `access = WRITE_ONLY`: a setter still binds this from a config source, but
    // `ContractGenerator`'s own `ObjectMapper().convertValue(new Config(), Map.class)` — used to
    // find each key's observed default for `Schema.withDefaultsFromValue` — never serialises it,
    // so this compiled default never reaches that map. The Java equivalent of the Rust example's
    // `SecretString` field carrying `#[serde(skip_serializing)]`: without it, the very real
    // default below would flow through as an *observed* default, and `ContractValidator`'s own
    // "a secret must not carry a default" refusal (`SecretWithDefaultException`) would then
    // refuse the contract this class renders — a secret's compiled fallback is real, but nothing
    // about the loader running with no input at all is something a published document should say
    // out loud.
    @JsonProperty(access = JsonProperty.Access.WRITE_ONLY)
    @Secret
    private String url = "postgres://localhost/orders_dev";

    /** Maximum number of pooled connections. */
    @JsonProperty("max_connections")
    private int maxConnections = 10;
}
