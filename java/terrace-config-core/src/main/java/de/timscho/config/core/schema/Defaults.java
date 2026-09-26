package de.timscho.config.core.schema;

import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import lombok.experimental.UtilityClass;
import org.jspecify.annotations.Nullable;

/**
 * Fills in each key's observed default from an already-assembled value — a port of the Rust
 * crate's {@code Schema::with_defaults_from_value}, wired onto {@link Schema#withDefaultsFromValue}.
 *
 * <p>{@code -core} has no Jackson runtime of its own by design (see {@code
 * terrace-config-core-jackson2}/{@code -jackson3}), so this class takes an already-nested {@code
 * Map<String, Object>} rather than a live configuration instance the way the Rust crate's
 * {@code with_defaults_from} does — the same escape hatch the Rust type itself falls back to for
 * a value it cannot {@code Serialize}. A caller with an {@code ObjectMapper} on its own classpath
 * gets the same result from {@code mapper.convertValue(instance, Map.class)}.
 */
@UtilityClass
public class Defaults {

    /** The same bound {@link Node} enforces, for the same reason: a value this deep is a stack overflow risk. */
    private static final int MAX_DEPTH = 32;

    public static Schema withDefaultsFromValue(final Schema schema, final Map<String, Object> root) {
        final List<Key> keys = new ArrayList<>(schema.getKeys().size());
        for (final Key key : schema.getKeys()) {
            // A required key has no default by definition: loading fails until something
            // supplies it, and whatever `root` happens to hold is an artefact of building the
            // value at all, not something worth publishing as a default.
            if (key.isRequired()) {
                keys.add(key);
                continue;
            }

            final Object observed = withoutUnset(find(root, key.getPath()), 0);
            if (observed == null && !containsPath(root, key.getPath())) {
                keys.add(key);
                continue;
            }

            final String rendered = renderValue(observed, 0);
            if (rendered == null) {
                keys.add(key);
                continue;
            }

            // Redaction after rendering, not before: a secret that is unset by default is not a
            // secret worth hiding, and `<redacted>` in place of "unset" would read as though the
            // service ships with a credential baked in.
            if (key.isSecret()) {
                // No refinement check here: the value is withheld, so a refinement applied later
                // could never see it either, and checking it on this path alone would make the
                // result depend on which ran first. A secret with a default is refused anyway.
                keys.add(key.toBuilder().defaultText("<redacted>").build());
                continue;
            }
            // A default the refined constraint rejects supplies nothing: the image refuses it at
            // boot. The rule `Refiner` applies when the default arrived first, so the two commute.
            if (Refiner.rejectedByRefinement(key, observed)) {
                keys.add(key.toBuilder().required(true).build());
                continue;
            }
            keys.add(
                    key.toBuilder().defaultText(rendered).defaultValue(observed).build());
        }
        return schema.toBuilder().keys(keys).build();
    }

    /**
     * {@code value} with every unset field of every table taken out — the Rust crate's {@code
     * without_unset}. A {@code null} inside a table is a field no TOML file can spell and no element
     * schema admits; what the loader sees for it is absence. An item of a list keeps its place.
     */
    private static @Nullable Object withoutUnset(@Nullable final Object value, final int depth) {
        if (depth > MAX_DEPTH) {
            return value;
        }
        if (value instanceof Map<?, ?> dict) {
            final Map<String, Object> kept = new java.util.TreeMap<>();
            for (final Map.Entry<?, ?> entry : dict.entrySet()) {
                if (entry.getValue() != null) {
                    kept.put(String.valueOf(entry.getKey()), withoutUnset(entry.getValue(), depth + 1));
                }
            }
            return kept;
        }
        if (value instanceof List<?> items) {
            final List<Object> kept = new ArrayList<>(items.size());
            for (final Object item : items) {
                kept.add(withoutUnset(item, depth + 1));
            }
            return kept;
        }
        return value;
    }

    /** The value at a dotted path, or {@code null} for one {@code root} does not carry — see {@link #containsPath}. */
    private static @Nullable Object find(final Map<String, Object> root, final String path) {
        Object current = root;
        for (final String segment : path.split("\\.")) {
            if (!(current instanceof Map<?, ?> map) || !map.containsKey(segment)) {
                return null;
            }
            current = map.get(segment);
        }
        return current;
    }

    /** Whether {@code root} carries a value at this path at all, distinguishing "absent" from "present but null". */
    private static boolean containsPath(final Map<String, Object> root, final String path) {
        Object current = root;
        final String[] segments = path.split("\\.");
        for (int i = 0; i < segments.length; i++) {
            if (!(current instanceof Map<?, ?> map) || !map.containsKey(segments[i])) {
                return false;
            }
            current = map.get(segments[i]);
        }
        return true;
    }

    /**
     * A default value as a table would show it, or {@code null} for one that means "absent" — an
     * explicit null and a missing key render the same way, which is what they mean to an operator.
     */
    private static @Nullable String renderValue(@Nullable final Object value, final int depth) {
        if (depth > MAX_DEPTH) {
            return "…";
        }
        if (value == null) {
            return null;
        }
        if (value instanceof String string) {
            // Quoted, so an empty default is distinguishable from an absent one in a rendered cell.
            return string.isEmpty() ? "\"\"" : string;
        }
        if (value instanceof Boolean || value instanceof Number) {
            return value.toString();
        }
        if (value instanceof List<?> items) {
            return renderList(items, depth);
        }
        if (value instanceof Map<?, ?> dict) {
            return renderMap(dict, depth);
        }
        return value.toString();
    }

    private static String renderList(final List<?> items, final int depth) {
        final List<String> rendered = new ArrayList<>(items.size());
        for (final Object item : items) {
            final String literal = renderValue(item, depth + 1);
            rendered.add(literal == null ? "unset" : literal);
        }
        return "[" + String.join(", ", rendered) + "]";
    }

    // A dict at a leaf means the field wanted `@Nested`. Rendered as an inline table rather than
    // dropped, so the output shows what is actually there.
    private static String renderMap(final Map<?, ?> dict, final int depth) {
        final List<String> rendered = new ArrayList<>();
        for (final Map.Entry<?, ?> entry : dict.entrySet()) {
            final String literal = renderValue(entry.getValue(), depth + 1);
            rendered.add(entry.getKey() + " = " + (literal == null ? "unset" : literal));
        }
        return "{ " + String.join(", ", rendered) + " }";
    }
}
