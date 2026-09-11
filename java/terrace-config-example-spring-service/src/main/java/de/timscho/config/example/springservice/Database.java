package de.timscho.config.example.springservice;

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
     * Connection string. A real deployment supplies this through file indirection, never through
     * a plain environment variable, so the compiled default below is the only value this field
     * is meant to ever hold in plain sight.
     */
    // Plain prose above, no `{@code}` markup: `Javadocs.normalize` carries a field's doc comment
    // into `contract.json` verbatim rather than rendering javadoc tags.
    @Secret
    private String url = "postgres://localhost/orders_dev";

    /** Maximum number of pooled connections. */
    private int maxConnections = 10;
}
