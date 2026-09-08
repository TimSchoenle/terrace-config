package de.timscho.config.core.descriptor;

import java.util.List;

/**
 * What a container-typed key's element looks like, one level down from {@link KeyDescriptor}. At
 * most one of {@code values} and {@code nestedKeys} is populated; {@code typeName} is always
 * present.
 *
 * @param typeName   the element's Java type, simple-named
 * @param values     the spellings the element accepts, from {@code @ElementValues}; empty if none
 * @param range      the bound on the element, from {@code @Range} combined with {@code @Element};
 *                   {@code null} if none
 * @param nestedKeys the element's own keys, from {@code @Element} onto a {@code @TerraceConfig}
 *                   struct; empty if the element is not itself a nested struct
 */
public record ElementDescriptor(
        String typeName,
        List<String> values,
        RangeConstraint range,
        List<KeyDescriptor> nestedKeys) {
}
