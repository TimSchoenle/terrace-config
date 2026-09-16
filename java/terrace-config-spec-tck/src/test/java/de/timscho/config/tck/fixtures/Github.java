package de.timscho.config.tck.fixtures;

import com.fasterxml.jackson.annotation.JsonAlias;
import com.fasterxml.jackson.annotation.JsonProperty;

import de.timscho.config.annotations.Note;
import de.timscho.config.annotations.Range;
import de.timscho.config.annotations.Secret;
import de.timscho.config.annotations.TerraceConfig;

/** The GitHub access {@link FullSurfaceConfig} nests under {@code github}. */
@TerraceConfig
public final class Github {

    // Plain prose in the Javadoc below, no `{@code}`/`{@link}` markup: `Javadocs.normalize`
    // carries a field's doc comment into `contract.json` verbatim rather than rendering javadoc
    // tags, so any markup here would reach that document as literal braces.
    /** User whose repositories are listed. */
    @JsonAlias("user")
    private String username;

    // `access = WRITE_ONLY`: a setter still binds this from a config source, but the
    // `ObjectMapper` conversion `FixtureContracts` uses to find each key's observed default never
    // serialises it, so this compiled placeholder never reaches that map — the same idiom
    // `terrace-config-example-service`'s `Database.url` uses, and for the same reason: a secret
    // carrying an observed default is one of the eight refusals a producer must raise (see
    // `de.timscho.config.core.refusal.SecretWithDefaultException`).
    /** Bearer token lifting the API rate limit. */
    @JsonProperty(access = JsonProperty.Access.WRITE_ONLY)
    @Secret
    private String token = "";

    /** Revalidation interval in seconds. */
    @JsonProperty("ttl_secs")
    @Note("permanent")
    @Range(min = 0.0)
    private long ttlSecs = 0L;

    public String getUsername() {
        return username;
    }

    public void setUsername(final String username) {
        this.username = username;
    }

    public String getToken() {
        return token;
    }

    public void setToken(final String token) {
        this.token = token;
    }

    public long getTtlSecs() {
        return ttlSecs;
    }

    public void setTtlSecs(final long ttlSecs) {
        this.ttlSecs = ttlSecs;
    }
}
