package de.timscho.config.example.service;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.Getter;
import lombok.Setter;

import de.timscho.config.annotations.Secret;
import de.timscho.config.annotations.TerraceConfig;

/** The database this service persists orders to. */
@TerraceConfig
@Getter
@Setter
public class Database {

    /**
     * Connection string. A real deployment supplies this through the secrets directory or file
     * indirection, never through a plain environment variable, so the compiled default below is
     * the only value this field is meant to ever hold in plain sight.
     */
    // Plain prose above, no `{@code}` markup: `Javadocs.normalize` carries a field's doc comment
    // into `contract.json` verbatim rather than rendering javadoc tags, so a `{@code _FILE}` here
    // would reach that document as the literal seven characters `{@code`.
    @Secret
    private String url = "postgres://localhost/orders_dev";

    /** Maximum number of pooled connections. */
    @JsonProperty("max_connections")
    private int maxConnections = 10;
}
