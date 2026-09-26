package de.timscho.config.core.schema;

import de.timscho.config.core.refusal.ConstraintEvaluator;
import java.math.BigDecimal;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import lombok.experimental.UtilityClass;
import org.jspecify.annotations.Nullable;

/**
 * A {@link Condition} checked against the struct it is stated on, as the JSON Schema that says it
 * and the sentence every rendering shows — the port of the Rust crate's {@code schema::condition}.
 * Same encodings, same sentences, same refusals.
 */
@UtilityClass
class Conditions {

    private static final int MAX_DEPTH = 32;

    /**
     * A condition stated: its schema, and its readable form.
     *
     * @param schema      the JSON Schema that says it
     * @param description the sentence every rendering shows
     */
    record Stated(Map<String, Object> schema, String description) {}

    /** Why a condition cannot be stated on a struct. */
    static final class Refused extends Exception {
        private static final long serialVersionUID = 1L;

        Refused(final String message) {
            super(message, null, false, false);
        }
    }

    static Stated state(final Condition condition, final Map<String, Object> target, final String at) throws Refused {
        return new Stated(schema(condition, target, at, 0), describe(condition));
    }

    private static Map<String, Object> schema(
            final Condition condition, final Map<String, Object> target, final String at, final int depth)
            throws Refused {
        if (depth > MAX_DEPTH) {
            throw new Refused("conditions nest more than " + MAX_DEPTH + " deep");
        }
        // CHECKSTYLE.OFF: Indentation -- palantirJavaFormat wraps each arrow-case body at 4 spaces
        // past `case`; the fetched ruleset's Indentation check wants 8 (see Refiner's note).
        return switch (condition) {
            case Condition.Present present -> {
                field(target, at, present.field());
                yield wrap(present.field(), new TreeMap<>(), true);
            }
            case Condition.Absent absent -> {
                field(target, at, absent.field());
                yield absent(absent.field());
            }
            case Condition.NonEmpty nonEmpty -> emptiness(target, at, nonEmpty.field(), true);
            case Condition.Empty empty -> emptiness(target, at, empty.field(), false);
            case Condition.Equals equals -> equality(target, at, equals.field(), equals.value(), true);
            case Condition.NotEquals notEquals -> equality(target, at, notEquals.field(), notEquals.value(), false);
            case Condition.Above above -> above(target, at, above);
            case Condition.Matches matches -> matching(target, at, matches);
            case Condition.All all -> all(all.conditions(), target, at, depth);
            case Condition.ExactlyOne exactlyOne -> {
                if (exactlyOne.conditions().size() < 2) {
                    throw new Refused("`exactly one` needs at least two conditions to choose between");
                }
                yield one("oneOf", nested(exactlyOne.conditions(), target, at, depth));
            }
            case Condition.When when -> {
                final Map<String, Object> schema = one("if", schema(when.condition(), target, at, depth + 1));
                schema.put("then", schema(when.then(), target, at, depth + 1));
                yield schema;
            }
        };
        // CHECKSTYLE.ON: Indentation
    }

    private static List<Object> nested(
            final List<Condition> conditions, final Map<String, Object> target, final String at, final int depth)
            throws Refused {
        final List<Object> schemas = new ArrayList<>();
        for (final Condition condition : conditions) {
            schemas.add(schema(condition, target, at, depth + 1));
        }
        return schemas;
    }

    private static Map<String, Object> all(
            final List<Condition> conditions, final Map<String, Object> target, final String at, final int depth)
            throws Refused {
        if (conditions.isEmpty()) {
            throw new Refused("`all` of no conditions states nothing");
        }
        if (conditions.size() == 1) {
            return schema(conditions.get(0), target, at, depth + 1);
        }
        return one("allOf", nested(conditions, target, at, depth));
    }

    private static Map<String, Object> emptiness(
            final Map<String, Object> target, final String at, final String path, final boolean nonEmpty)
            throws Refused {
        final Map<?, ?> held = field(target, at, path);
        final String keyword;
        if (isType(held, "object")) {
            keyword = nonEmpty ? "minProperties" : "maxProperties";
        } else if (isType(held, "array")) {
            keyword = nonEmpty ? "minItems" : "maxItems";
        } else if (isType(held, "string")) {
            keyword = nonEmpty ? "minLength" : "maxLength";
        } else {
            throw new Refused("`" + path + "` is neither a map, a sequence nor a string, so it has no emptiness: its "
                    + "constraint is " + ConstraintEvaluator.show(held));
        }
        return wrap(path, one(keyword, nonEmpty ? 1 : 0), nonEmpty);
    }

    private static Map<String, Object> equality(
            final Map<String, Object> target,
            final String at,
            final String path,
            @Nullable final Object value,
            final boolean equal)
            throws Refused {
        final Map<?, ?> held = field(target, at, path);
        if (ConstraintEvaluator.verdict(copy(held), value) instanceof ConstraintEvaluator.Fails failed) {
            throw new Refused("`" + path + "` can never be " + json(value) + ", which its own constraint rejects: "
                    + failed.reason());
        }
        final Map<String, Object> leaf = equal ? one("const", value) : one("not", one("const", value));
        return wrap(path, leaf, true);
    }

    private static Map<String, Object> above(
            final Map<String, Object> target, final String at, final Condition.Above above) throws Refused {
        final Map<?, ?> held = field(target, at, above.field());
        if (!isType(held, "integer") && !isType(held, "number")) {
            throw new Refused("`" + above.field() + "` is not a number, so it has nothing to be above "
                    + json(above.bound()) + ": its constraint is " + ConstraintEvaluator.show(held));
        }
        return wrap(above.field(), one("exclusiveMinimum", above.bound()), true);
    }

    private static Map<String, Object> matching(
            final Map<String, Object> target, final String at, final Condition.Matches matches) throws Refused {
        final Map<?, ?> held = field(target, at, matches.field());
        if (!isType(held, "string")) {
            throw new Refused("`" + matches.field() + "` is not a string, so it has no text for a pattern to match: "
                    + "its constraint is " + ConstraintEvaluator.show(held));
        }
        final String refused = PortablePattern.refusal(matches.pattern());
        if (refused != null) {
            throw new Refused("`" + matches.pattern() + "` cannot be matched against `" + matches.field() + "`: "
                    + refused + ". See *Portable patterns* in spec/v1/FORMAT.md");
        }
        return wrap(matches.field(), one("pattern", matches.pattern()), true);
    }

    /** The schema of the field {@code path} names below the struct at {@code at}. */
    private static Map<?, ?> field(final Map<String, Object> target, final String at, final String path)
            throws Refused {
        Map<?, ?> schema = target;
        final StringBuilder reached = new StringBuilder(at);
        for (final String segment : path.split("\\.", -1)) {
            final Object declared = schema.get("properties");
            if (declared instanceof Map<?, ?> fields && fields.get(segment) instanceof Map<?, ?> next) {
                schema = next;
                reached.append('.').append(segment);
                continue;
            }
            final StringBuilder names = new StringBuilder();
            if (declared instanceof Map<?, ?> fields) {
                for (final Object name : new TreeMap<>(fields).keySet()) {
                    names.append(names.isEmpty() ? "" : ", ")
                            .append('`')
                            .append(name)
                            .append('`');
                }
            }
            throw new Refused("`" + segment + "` is not a field of `" + reached + "`, so a condition on `" + path
                    + "` names nothing."
                    + (declared instanceof Map<?, ?> ? " Its fields are " + names + "." : " It declares no fields."));
        }
        return schema;
    }

    private static boolean isType(final Map<?, ?> held, final String name) {
        final Object type = held.get("type");
        return name.equals(type) || (type instanceof List<?> names && names.contains(name));
    }

    private static Map<String, Object> one(final String keyword, @Nullable final Object value) {
        final Map<String, Object> schema = new TreeMap<>();
        schema.put(keyword, value);
        return schema;
    }

    /** {@code leaf} at {@code path}, nested through each segment, every segment required when {@code present}. */
    private static Map<String, Object> wrap(final String path, final Map<String, Object> leaf, final boolean present) {
        final String[] segments = path.split("\\.", -1);
        Map<String, Object> inner = leaf;
        for (int index = segments.length - 1; index >= 0; index--) {
            final Map<String, Object> level = new TreeMap<>();
            if (present) {
                level.put("required", List.of(segments[index]));
            }
            if (!inner.isEmpty()) {
                level.put("properties", one(segments[index], inner));
            }
            inner = level;
        }
        return inner;
    }

    /** The field at {@code path} is not set: its parent, if present, does not hold it. */
    private static Map<String, Object> absent(final String path) {
        final int split = path.lastIndexOf('.');
        final String last = split < 0 ? path : path.substring(split + 1);
        final Map<String, Object> leaf = one("not", one("required", List.of(last)));
        return split < 0 ? leaf : wrap(path.substring(0, split), leaf, false);
    }

    /**
     * The sentence every rendering shows for {@code condition} — the Rust crate's, word for word.
     *
     * @param condition the condition
     */
    static String describe(final Condition condition) {
        // CHECKSTYLE.OFF: Indentation -- palantirJavaFormat wraps each arrow-case body at 4 spaces
        // past `case`; the fetched ruleset's Indentation check wants 8 (see Refiner's note).
        return switch (condition) {
            case Condition.Present present -> "`" + present.field() + "` is set";
            case Condition.Absent absent -> "`" + absent.field() + "` is not set";
            case Condition.NonEmpty nonEmpty -> "`" + nonEmpty.field() + "` is not empty";
            case Condition.Empty empty -> "`" + empty.field() + "` is empty or not set";
            case Condition.Equals equals -> "`" + equals.field() + "` is " + json(equals.value());
            case Condition.NotEquals notEquals ->
                "`" + notEquals.field() + "` is set and not " + json(notEquals.value());
            case Condition.Above above -> "`" + above.field() + "` is above " + json(above.bound());
            case Condition.Matches matches -> "`" + matches.field() + "` matches `" + matches.pattern() + "`";
            case Condition.All all ->
                all.conditions().size() == 1
                        ? describe(all.conditions().get(0))
                        : "all of: [" + list(all.conditions()) + "]";
            case Condition.ExactlyOne exactlyOne -> "exactly one of: [" + list(exactlyOne.conditions()) + "]";
            case Condition.When when -> "when " + describe(when.condition()) + ": " + describe(when.then());
        };
        // CHECKSTYLE.ON: Indentation
    }

    private static String list(final List<Condition> conditions) {
        final StringBuilder said = new StringBuilder();
        for (final Condition condition : conditions) {
            said.append(said.isEmpty() ? "" : "; ").append(describe(condition));
        }
        return said.toString();
    }

    /** A value as compact JSON, as the Rust crate's {@code serde_json::Value} displays it. */
    static String json(@Nullable final Object value) {
        if (value == null) {
            return "null";
        }
        if (value instanceof String text) {
            return quoted(text);
        }
        if (value instanceof BigDecimal decimal) {
            return decimal.stripTrailingZeros().toPlainString();
        }
        if (value instanceof List<?> items) {
            final StringBuilder out = new StringBuilder("[");
            for (final Object item : items) {
                out.append(out.length() > 1 ? "," : "").append(json(item));
            }
            return out.append(']').toString();
        }
        if (value instanceof Map<?, ?> fields) {
            final StringBuilder out = new StringBuilder("{");
            for (final Map.Entry<?, ?> field : new TreeMap<>(fields).entrySet()) {
                out.append(out.length() > 1 ? "," : "")
                        .append(json(String.valueOf(field.getKey())))
                        .append(':')
                        .append(json(field.getValue()));
            }
            return out.append('}').toString();
        }
        return String.valueOf(value);
    }

    /** A string as a JSON string literal. */
    private static String quoted(final String text) {
        final StringBuilder quoted = new StringBuilder("\"");
        for (final char c : text.toCharArray()) {
            if (c == '"' || c == '\\') {
                quoted.append('\\').append(c);
            } else if (c == '\n') {
                quoted.append("\\n");
            } else if (c == '\r') {
                quoted.append("\\r");
            } else if (c == '\t') {
                quoted.append("\\t");
            } else if (c < 0x20) {
                quoted.append(String.format("\\u%04x", (int) c));
            } else {
                quoted.append(c);
            }
        }
        return quoted.append('"').toString();
    }

    private static Map<String, Object> copy(final Map<?, ?> schema) {
        final Map<String, Object> copied = new TreeMap<>();
        for (final Map.Entry<?, ?> keyword : schema.entrySet()) {
            copied.put(String.valueOf(keyword.getKey()), keyword.getValue());
        }
        return copied;
    }
}
