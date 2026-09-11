package de.timscho.config.core.schema;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;

import lombok.experimental.UtilityClass;

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

    public static Schema withDefaultsFromValue(Schema schema, Map<String, Object> root) {
        List<Key> keys = new ArrayList<>(schema.getKeys().size());
        for (Key key : schema.getKeys()) {
            // A required key has no default by definition: loading fails until something
            // supplies it, and whatever `root` happens to hold is an artefact of building the
            // value at all, not something worth publishing as a default.
            if (key.isRequired()) {
                keys.add(key);
                continue;
            }

            Object observed = find(root, key.getPath());
            if (observed == null && !containsPath(root, key.getPath())) {
                keys.add(key);
                continue;
            }

            String rendered = renderValue(observed, 0);
            if (rendered == null) {
                keys.add(key);
                continue;
            }

            // Redaction after rendering, not before: a secret that is unset by default is not a
            // secret worth hiding, and `<redacted>` in place of "unset" would read as though the
            // service ships with a credential baked in.
            if (key.isSecret()) {
                keys.add(key.toBuilder().defaultText("<redacted>").build());
                continue;
            }
            keys.add(key.toBuilder().defaultText(rendered).defaultValue(observed).build());
        }
        return schema.toBuilder().keys(keys).build();
    }

    /** The value at a dotted path, or {@code null} for one {@code root} does not carry — see {@link #containsPath}. */
    private static Object find(Map<String, Object> root, String path) {
        Object current = root;
        for (String segment : path.split("\\.")) {
            if (!(current instanceof Map<?, ?> map) || !map.containsKey(segment)) {
                return null;
            }
            current = map.get(segment);
        }
        return current;
    }

    /** Whether {@code root} carries a value at this path at all, distinguishing "absent" from "present but null". */
    private static boolean containsPath(Map<String, Object> root, String path) {
        Object current = root;
        String[] segments = path.split("\\.");
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
    private static String renderValue(Object value, int depth) {
        if (depth > MAX_DEPTH) {
            return "\u2026";
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
            List<String> rendered = new ArrayList<>(items.size());
            for (Object item : items) {
                String literal = renderValue(item, depth + 1);
                rendered.add(literal == null ? "unset" : literal);
            }
            return "[" + String.join(", ", rendered) + "]";
        }
        if (value instanceof Map<?, ?> dict) {
            // A dict at a leaf means the field wanted `@Nested`. Rendered as an inline table
            // rather than dropped, so the output shows what is actually there.
            List<String> rendered = new ArrayList<>();
            for (Map.Entry<?, ?> entry : dict.entrySet()) {
                String literal = renderValue(entry.getValue(), depth + 1);
                rendered.add(entry.getKey() + " = " + (literal == null ? "unset" : literal));
            }
            return "{ " + String.join(", ", rendered) + " }";
        }
        return value.toString();
    }
}
