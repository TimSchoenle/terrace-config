package de.timscho.config.tck.fixtures;

import com.fasterxml.jackson.annotation.JsonProperty;
import de.timscho.config.annotations.TerraceConfig;

/**
 * The {@code minimal} spec case ({@code spec/v1/conformance/minimal/}), written as a real
 * {@code @TerraceConfig} type instead of restated by hand: one optional field with a default,
 * nothing else. {@code FixtureContracts.minimal()} assembles this through the real loader and
 * codec, exactly as {@code terrace-config-example-service}'s {@code Config} does for its own
 * service — the whole point of checking a Contract this way is that it can never describe a
 * field this type no longer has.
 */
@TerraceConfig
public final class MinimalConfig {

    /** Bundle directory the readiness probe checks. */
    @JsonProperty("dist_dir")
    private String distDir = "public";

    public String getDistDir() {
        return distDir;
    }

    public void setDistDir(final String distDir) {
        this.distDir = distDir;
    }
}
