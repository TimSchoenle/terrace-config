package de.timscho.config.tck.fixtures;

import com.fasterxml.jackson.annotation.JsonProperty;
import de.timscho.config.annotations.Nested;
import de.timscho.config.annotations.TerraceConfig;

/**
 * The {@code required-entries} spec case ({@code spec/v1/conformance/required-entries/}): a host
 * mounting a legal-pages library whose runtime check no type states. {@code
 * FixtureContracts.requiredEntries()} refines {@link LegalPages}' two maps with the entries the
 * library refuses to start without, relative to where this host mounts it.
 */
@TerraceConfig
public final class RequiredEntriesConfig {

    /** Bundle directory the readiness probe checks. */
    @JsonProperty("dist_dir")
    private String distDir = "public";

    /** The legal-pages library's configuration, mounted under legal. */
    @Nested
    private LegalPages legal = new LegalPages();

    public String getDistDir() {
        return distDir;
    }

    public void setDistDir(final String distDir) {
        this.distDir = distDir;
    }

    public LegalPages getLegal() {
        return legal;
    }

    public void setLegal(final LegalPages legal) {
        this.legal = legal;
    }
}
