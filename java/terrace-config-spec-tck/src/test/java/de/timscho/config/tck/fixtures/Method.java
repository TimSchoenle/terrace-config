package de.timscho.config.tck.fixtures;

/**
 * An HTTP method {@link FullSurfaceConfig#methods} admits. A plain enum, not a
 * {@code @TerraceConfig} one: {@link FullSurfaceConfig#methods} reports its accepted spellings
 * through {@code @ElementValues}'s literal-list form instead, which is what a foreign enum with
 * no annotations of its own has to use — see {@code de.timscho.config.annotations.ElementValues}.
 */
public enum Method {
    GET,
    POST
}
