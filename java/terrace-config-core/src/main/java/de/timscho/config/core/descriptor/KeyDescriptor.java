package de.timscho.config.core.descriptor;

import java.util.List;

import org.jspecify.annotations.Nullable;

/**
 * One field of a {@code @TerraceConfig}-annotated type, described independently of any dialect —
 * no environment spelling, no alias derivation, no {@code text_form}. A loader or Spring producer
 * combines this with its own dialect to build the actual {@code Key} model in {@link
 * de.timscho.config.core.model}.
 *
 * @param name       the field's own name, as declared — the last segment of the eventual key path
 * @param docs       the field's full Javadoc, verbatim; empty if none
 * @param summary    the first paragraph of {@code docs}, matching rustdoc's own summary convention
 * @param typeName   the field's Java type, simple-named (e.g. {@code "String"}, {@code
 *                   "List<String>"}); always present, even when {@code container} is not {@code
 *                   NONE}, in which case it names the container itself
 * @param container  which of {@code Optional}/{@code List}/{@code Set}/{@code Map} wraps the field,
 *                   or {@code NONE} for a bare field
 * @param secret     whether {@code @Secret} is present
 * @param note       the {@code @Note} prose, or {@code null} if none
 * @param values     the spellings the field accepts, from {@code @Values} or the field's own {@code
 *                   @TerraceConfig}-annotated enum type; empty if none
 * @param range      the bound on the field, from {@code @Range}; {@code null} if none
 * @param nestedKeys the field's own nested keys, from {@code @Nested} onto a {@code
 *                   @TerraceConfig} struct; empty if the field is not itself nested
 * @param element    what the container's element looks like, from {@code @Element}/{@code
 *                   @ElementValues}; {@code null} when {@code container} is {@code NONE} or the
 *                   element is a plain leaf needing no description of its own
 * @param closed     whether the field's own type is closed to unknown properties (read off {@code
 *                   @JsonIgnoreProperties(ignoreUnknown = false)}), meaningful only when {@code
 *                   nestedKeys} or {@code element} carries a nested struct
 */
public record KeyDescriptor(
        String name,
        String docs,
        String summary,
        String typeName,
        ContainerKind container,
        boolean secret,
        @Nullable String note,
        List<String> values,
        @Nullable RangeConstraint range,
        List<KeyDescriptor> nestedKeys,
        @Nullable ElementDescriptor element,
        boolean closed) {

    /** Which container, if any, wraps a field's declared type. */
    public enum ContainerKind {
        NONE,
        OPTIONAL,
        LIST,
        SET,
        MAP
    }
}
