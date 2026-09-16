package de.timscho.config.tck.fixtures;

import de.timscho.config.annotations.Range;
import de.timscho.config.annotations.TerraceConfig;

/**
 * The {@code unnameable-key} spec case ({@code spec/v1/conformance/unnameable-key/}): two fields
 * left in their bare camelCase spelling, deliberately without a {@code @JsonProperty} rename.
 *
 * <p>{@code distDir} upper-cased is {@code DISTDIR}, which a case-folding environment reader maps
 * back to {@code distdir} — not {@code distDir} — so no environment variable can actually name
 * it. {@link de.timscho.config.core.descriptor.SchemaAssembler}'s own round-trip check is what
 * catches this (see its {@code envSpelling}), the same rule the Rust reference implementation
 * applies to a struct field it renders under {@code #[serde(rename_all = "camelCase")]}: both
 * publish the key with {@code env: null} and {@code unreachable: "unnameable"} rather than
 * silently picking a spelling nothing actually reads.
 */
@TerraceConfig
public final class UnnameableConfig {

    /** Where the bundle is read from. */
    private String distDir = "public";

    /** How long a rendered page stays fresh. */
    @Range(min = 0.0)
    private long maxAgeSecs = 0L;

    public String getDistDir() {
        return distDir;
    }

    public void setDistDir(final String distDir) {
        this.distDir = distDir;
    }

    public long getMaxAgeSecs() {
        return maxAgeSecs;
    }

    public void setMaxAgeSecs(final long maxAgeSecs) {
        this.maxAgeSecs = maxAgeSecs;
    }
}
