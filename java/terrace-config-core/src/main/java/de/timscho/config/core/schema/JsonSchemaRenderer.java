package de.timscho.config.core.schema;

import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import lombok.experimental.UtilityClass;
import org.jspecify.annotations.Nullable;

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

    /** The schema as a JSON Schema document, nested {@code properties} object per path level.
     *
     * @param schema  the schema to render
     * @param options which dialect, title, and strictness to render with
     */
    public static Map<String, Object> document(final Schema schema, final JsonSchemaOptions options) {
        final List<Key> reachable = new ArrayList<>();
        for (final Key key : schema.getKeys()) {
            if (!key.isReserved()) {
                reachable.add(key);
            }
        }
        // No closed levels here: whether an undeclared key is an error at the document's own
        // levels is `options.closed()`'s question, not a type's.
        final Map<String, Object> document = object(Node.of(reachable), "", options, Collections.emptySet());

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
    private static Map<String, Object> object(
            final Node node, final String path, final JsonSchemaOptions options, final Set<String> closed) {
        final Map<String, Object> properties = new TreeMap<>();
        final List<Object> required = new ArrayList<>();
        // A required key with aliases is a *choice* of properties, one of which must be present —
        // `required` cannot say that, so those collect here under one `allOf` instead.
        final List<Object> either = new ArrayList<>();

        addKeyProperties(node, options, properties, required, either);
        // After the leaves, so the one shape neither format can carry — a key and a table of the
        // same name in one parent — resolves to the table.
        addChildProperties(node, path, options, closed, properties, required);

        final Map<String, Object> objectSchema = new TreeMap<>();
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

    /** This level's own keys, each as a {@code properties} entry — plus its aliases (each their
     * own entry, cross-referencing the same schema) and, for a required key, either a plain
     * {@code required} name or an {@code anyOf} of every spelling. */
    private static void addKeyProperties(
            final Node node,
            final JsonSchemaOptions options,
            final Map<String, Object> properties,
            final List<Object> required,
            final List<Object> either) {
        for (final Key key : node.keys) {
            final String name = Node.name(key.getPath());
            final Map<String, Object> keySchema = leaf(key, options);

            for (final String alias : key.getAliases()) {
                final Map<String, Object> spelling = new TreeMap<>(keySchema);
                spelling.put("description", "Another spelling of `" + key.getPath() + "`.");
                properties.put(Node.name(alias), spelling);
            }

            properties.put(name, keySchema);

            if (key.isRequired() && options.requirePresent()) {
                if (key.getAliases().isEmpty()) {
                    required.add(name);
                } else {
                    final List<Object> spellings = new ArrayList<>();
                    spellings.add(requiredOne(name));
                    for (final String alias : key.getAliases()) {
                        spellings.add(requiredOne(Node.name(alias)));
                    }
                    final Map<String, Object> anyOf = new TreeMap<>();
                    anyOf.put("anyOf", spellings);
                    either.add(anyOf);
                }
            }
        }
    }

    /** This level's own nested tables, each rendered recursively as a {@code properties} entry. */
    private static void addChildProperties(
            final Node node,
            final String path,
            final JsonSchemaOptions options,
            final Set<String> closed,
            final Map<String, Object> properties,
            final List<Object> required) {
        for (final Node child : node.children) {
            final String childPath = path.isEmpty() ? child.segment : path + "." + child.segment;
            properties.put(child.segment, object(child, childPath, options, closed));
            if (child.required() && options.requirePresent()) {
                required.add(child.segment);
            }
        }
    }

    private static Map<String, Object> requiredOne(final String name) {
        final Map<String, Object> requiredOne = new TreeMap<>();
        requiredOne.put("required", List.of(name));
        return requiredOne;
    }

    /** One key as a JSON Schema subschema. */
    private static Map<String, Object> leaf(final Key key, final JsonSchemaOptions options) {
        final Map<String, Object> schema = new TreeMap<>();

        final String description = description(key, options);
        if (description != null) {
            schema.put("description", description);
        }

        // Read off the key rather than re-derived: the key's own `constraint` already composed
        // the type's shape with whatever element schema a container-typed key reported.
        if (key.getConstraint() != null) {
            final Map<String, Object> constraint = deepCopy(key.getConstraint());
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
    private static @Nullable String description(final Key key, final JsonSchemaOptions options) {
        final String docs = options.docs().of(key.getDocs());
        final String note = key.getNote() != null ? "Default: " + key.getNote() + "." : null;
        if (docs != null && note != null) {
            return docs + "\n\n" + note;
        }
        return docs != null ? docs : note;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> deepCopy(final Map<String, Object> source) {
        final Map<String, Object> copy = new TreeMap<>();
        for (final Map.Entry<String, Object> entry : source.entrySet()) {
            copy.put(entry.getKey(), deepCopyValue(entry.getValue()));
        }
        return copy;
    }

    @SuppressWarnings("unchecked")
    private static Object deepCopyValue(final Object value) {
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
    private static void close(final Map<String, Object> schema) {
        final Object items = schema.get("items");
        if (items instanceof Map) {
            close((Map<String, Object>) items);
        }
        final Object additionalProperties = schema.get("additionalProperties");
        if (additionalProperties instanceof Map) {
            close((Map<String, Object>) additionalProperties);
            return;
        }
        final Object properties = schema.get("properties");
        if (!(properties instanceof Map)) {
            return;
        }
        for (final Object property : ((Map<String, Object>) properties).values()) {
            if (property instanceof Map) {
                close((Map<String, Object>) property);
            }
        }
        schema.put("additionalProperties", false);
    }
}
