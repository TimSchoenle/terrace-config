package de.timscho.config.core.schema;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;

import lombok.experimental.UtilityClass;
import org.jspecify.annotations.Nullable;

import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;

/**
 * The JSON Schema rendering: what an editor or a Helm chart validates a rendered document
 * against — ported from the Rust crate's {@code schema::json_schema} module.
 *
 * <p>Every map returned is a {@link TreeMap}, so its own iteration order is already alphabetical
 * — the same order {@code serde_json}'s default (non-{@code preserve_order}) map produces in the
 * Rust crate's own output, which is what every stored {@code contract.json}'s {@code json_schema}
 * half is written in.
 *
 * <p>See {@code rust/src/schema/json_schema.rs} for the full rationale behind each keyword this
 * emits; this port carries the same restraint: a keyword is only emitted where it is certainly
 * true of every value the loader would accept.
 */
@UtilityClass
public class JsonSchemaRenderer {

    /** The schema as a JSON Schema document, nested {@code properties} object per path level. */
    public static Map<String, Object> document(Schema schema, JsonSchemaOptions options) {
        List<Key> reachable = new ArrayList<>();
        for (Key key : schema.getKeys()) {
            if (!key.isReserved()) {
                reachable.add(key);
            }
        }
        // No closed levels here: whether an undeclared key is an error at the document's own
        // levels is `options.closed()`'s question, not a type's.
        Map<String, Object> document = object(Node.of(reachable), "", options, Collections.emptySet());

        document.put("$schema", options.metaSchema());
        if (options.id() != null) {
            document.put("$id", options.id());
        }
        if (options.title() != null) {
            document.put("title", options.title());
        }
        return document;
    }

    /**
     * One level of the configuration as a JSON Schema object. {@code path} is where this level
     * sits, dotted, empty at the root.
     */
    private static Map<String, Object> object(Node node, String path, JsonSchemaOptions options, Set<String> closed) {
        Map<String, Object> properties = new TreeMap<>();
        List<Object> required = new ArrayList<>();
        // A required key with aliases is a *choice* of properties, one of which must be present —
        // `required` cannot say that, so those collect here under one `allOf` instead.
        List<Object> either = new ArrayList<>();

        for (Key key : node.keys) {
            String name = Node.name(key.getPath());
            Map<String, Object> keySchema = leaf(key, options);

            for (String alias : key.getAliases()) {
                Map<String, Object> spelling = new TreeMap<>(keySchema);
                spelling.put("description", "Another spelling of `" + key.getPath() + "`.");
                properties.put(Node.name(alias), spelling);
            }

            properties.put(name, keySchema);

            if (key.isRequired() && options.requirePresent()) {
                if (key.getAliases().isEmpty()) {
                    required.add(name);
                } else {
                    List<Object> spellings = new ArrayList<>();
                    spellings.add(requiredOne(name));
                    for (String alias : key.getAliases()) {
                        spellings.add(requiredOne(Node.name(alias)));
                    }
                    Map<String, Object> anyOf = new TreeMap<>();
                    anyOf.put("anyOf", spellings);
                    either.add(anyOf);
                }
            }
        }
        // After the leaves, so the one shape neither format can carry — a key and a table of the
        // same name in one parent — resolves to the table.
        for (Node child : node.children) {
            String childPath = path.isEmpty() ? child.segment : path + "." + child.segment;
            properties.put(child.segment, object(child, childPath, options, closed));
            if (child.required() && options.requirePresent()) {
                required.add(child.segment);
            }
        }

        Map<String, Object> objectSchema = new TreeMap<>();
        objectSchema.put("type", "object");
        objectSchema.put("properties", properties);
        if (!required.isEmpty()) {
            objectSchema.put("required", required);
        }
        if (!either.isEmpty()) {
            objectSchema.put("allOf", either);
        }
        if (options.closed() || closed.contains(path)) {
            objectSchema.put("additionalProperties", false);
        }
        return objectSchema;
    }

    private static Map<String, Object> requiredOne(String name) {
        Map<String, Object> requiredOne = new TreeMap<>();
        requiredOne.put("required", List.of(name));
        return requiredOne;
    }

    /** One key as a JSON Schema subschema. */
    private static Map<String, Object> leaf(Key key, JsonSchemaOptions options) {
        Map<String, Object> schema = new TreeMap<>();

        String description = description(key, options);
        if (description != null) {
            schema.put("description", description);
        }

        // Read off the key rather than re-derived: the key's own `constraint` already composed
        // the type's shape with whatever element schema a container-typed key reported.
        if (key.getConstraint() != null) {
            Map<String, Object> constraint = deepCopy(key.getConstraint());
            if (options.closed()) {
                close(constraint);
            }
            schema.putAll(constraint);
        }

        if (key.isSecret()) {
            // As close as JSON Schema comes to saying "credential" — a value written but never
            // read back.
            schema.put("writeOnly", true);
        }

        if (options.defaults() && !key.isSecret() && key.getDefaultValue() != null) {
            schema.put("default", key.getDefaultValue());
        }

        return schema;
    }

    /** The {@code description} for a key: its comment, and what its default means. */
    private static @Nullable String description(Key key, JsonSchemaOptions options) {
        String docs = options.docs().of(key.getDocs());
        String note = key.getNote() != null ? "Default: " + key.getNote() + "." : null;
        if (docs != null && note != null) {
            return docs + "\n\n" + note;
        }
        return docs != null ? docs : note;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> deepCopy(Map<String, Object> source) {
        Map<String, Object> copy = new TreeMap<>();
        for (Map.Entry<String, Object> entry : source.entrySet()) {
            copy.put(entry.getKey(), deepCopyValue(entry.getValue()));
        }
        return copy;
    }

    @SuppressWarnings("unchecked")
    private static Object deepCopyValue(Object value) {
        if (value instanceof Map) {
            return deepCopy((Map<String, Object>) value);
        }
        if (value instanceof List) {
            return new ArrayList<>((List<Object>) value);
        }
        return value;
    }

    /**
     * Close every object inside a rendered constraint, the way {@link #object} closes the
     * document's own levels — only where a schema declares {@code properties} and says nothing
     * yet about the rest, and recursing into {@code items}/{@code additionalProperties} rather
     * than overwriting either, since those are schemas of their own.
     */
    @SuppressWarnings("unchecked")
    private static void close(Map<String, Object> schema) {
        Object items = schema.get("items");
        if (items instanceof Map) {
            close((Map<String, Object>) items);
        }
        Object additionalProperties = schema.get("additionalProperties");
        if (additionalProperties instanceof Map) {
            close((Map<String, Object>) additionalProperties);
            return;
        }
        Object properties = schema.get("properties");
        if (!(properties instanceof Map)) {
            return;
        }
        for (Object property : ((Map<String, Object>) properties).values()) {
            if (property instanceof Map) {
                close((Map<String, Object>) property);
            }
        }
        schema.put("additionalProperties", false);
    }
}
