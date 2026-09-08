package de.timscho.config.core.schema;

import java.util.ArrayList;
import java.util.List;

import de.timscho.config.core.model.Key;

/**
 * One level of the configuration: the keys directly under a path, and the levels below it —
 * ported from the Rust crate's {@code tree::Node}, which the Markdown table has no use for but
 * both a TOML and a JSON Schema rendering share, so the two cannot disagree about which keys
 * belong to which table.
 *
 * <p>Order is first-appearance order at every level, which is declaration order for a schema
 * built from one type. Package-private: only {@link JsonSchemaRenderer} builds and walks one.
 */
final class Node {

    // Explicit `public` rather than the no-modifier default: `Node` itself is package-private, so
    // visibility is already confined to `de.timscho.config.core.schema`; an explicit modifier is
    // needed only because `java/lombok.config`'s `lombok.fieldDefaults.defaultPrivate` would
    // otherwise make a no-modifier field private, which `JsonSchemaRenderer` cannot then read.

    /** This level's own path segment. Empty at the root. */
    public final String segment;

    /** The keys that live directly at this level, in the order they were described. */
    public final List<Key> keys = new ArrayList<>();

    /** The levels below this one, in the order they were first reached. */
    public final List<Node> children = new ArrayList<>();

    private Node(String segment) {
        this.segment = segment;
    }

    /**
     * Group {@code keys} by the path each one carries, split on {@code .} — the same split a
     * dialect's nesting separator ultimately maps back onto.
     */
    static Node of(List<Key> keys) {
        Node root = new Node("");
        for (Key key : keys) {
            root.insert(key.getPath().split("\\.", -1), 0, key);
        }
        return root;
    }

    private void insert(String[] segments, int index, Key key) {
        if (index == segments.length - 1) {
            keys.add(key);
            return;
        }
        String head = segments[index];
        Node child = null;
        for (Node candidate : children) {
            if (candidate.segment.equals(head)) {
                child = candidate;
                break;
            }
        }
        if (child == null) {
            child = new Node(head);
            children.add(child);
        }
        child.insert(segments, index + 1, key);
    }

    /**
     * Whether anything at or below this level must be supplied. A table is required exactly
     * when it holds a required key, however deep.
     */
    boolean required() {
        for (Key key : keys) {
            if (key.isRequired()) {
                return true;
            }
        }
        for (Node child : children) {
            if (child.required()) {
                return true;
            }
        }
        return false;
    }

    /**
     * Whether a level of this name opens below this one.
     *
     * <p>The one case where a key and a table collide: a path described as a leaf <i>and</i> a
     * nested struct beside it -- a field that wanted {@code @Nested} and did not get it. TOML
     * cannot express both, so a renderer has to know.
     */
    boolean opens(String segment) {
        for (Node child : children) {
            if (child.segment.equals(segment)) {
                return true;
            }
        }
        return false;
    }

    /** The last segment of a dotted path — the key's own name, as a file spells it. */
    static String name(String path) {
        int lastDot = path.lastIndexOf('.');
        return lastDot < 0 ? path : path.substring(lastDot + 1);
    }
}
