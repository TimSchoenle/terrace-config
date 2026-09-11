package de.timscho.config.core.descriptor;

import java.util.List;

/**
 * The whole of what {@code terrace-config-processor} reads off one {@code @TerraceConfig}-
 * annotated type — a struct's keys, or an enum's accepted spellings. Generated as a {@code
 * public static final TypeDescriptor DESCRIPTOR} field on a {@code <Type>Descriptor} class beside
 * the annotated type.
 *
 * @param qualifiedName the annotated type's fully qualified name
 * @param kind          {@link Kind#STRUCT} for a class or record, {@link Kind#ENUM} for an enum
 * @param keys          the type's keys, in declaration order; empty for {@link Kind#ENUM}
 * @param values        the enum's constants' spellings, in declaration order; empty for {@link
 *                      Kind#STRUCT}
 * @param closed        whether the type is closed to unknown properties (read off {@code
 *                      @JsonIgnoreProperties(ignoreUnknown = false)}); meaningless for an enum
 */
public record TypeDescriptor(
        String qualifiedName, Kind kind, List<KeyDescriptor> keys, List<String> values, boolean closed) {

    /** Whether a described type is a struct (keys of its own) or an enum (values one key accepts). */
    public enum Kind {
        STRUCT,
        ENUM
    }
}
