package de.timscho.config.loader;

/**
 * A loaded config together with what a reload needs to watch and to compare against. The Java
 * equivalent of the Rust crate's {@code loaded::Loaded}, returned by {@link
 * TerraceLoader#loadWatched}.
 *
 * @param value the extracted config
 * @param sources where it came from
 * @param <T> the config type
 */
public record Loaded<T>(T value, Sources sources) {}
