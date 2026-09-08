package de.timscho.config.loader;

/**
 * A loaded config together with what a reload needs to watch and to compare against — the Java
 * equivalent of the Rust crate's {@code loaded::Loaded}, returned by
 * {@link TerraceLoader#loadWatched}.
 *
 * @param value the extracted config
 * @param sources where it came from
 */
public record Loaded<T>(T value, Sources sources) {
}
