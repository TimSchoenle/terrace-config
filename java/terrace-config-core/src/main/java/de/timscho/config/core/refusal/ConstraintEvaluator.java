package de.timscho.config.core.refusal;

import de.timscho.config.core.schema.PortablePattern;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Pattern;
import org.jspecify.annotations.Nullable;

/**
 * Whether a value satisfies a published constraint, as far as this module can prove it — the port
 * of the Rust crate's {@code schema::check}, and what refusal 9 rests on.
 *
 * <p>{@code -core} links no JSON Schema engine, so this is a small one over exactly the vocabulary
 * a producer here emits into a constraint, written to the rule the constraint itself is: a refusal
 * must be certain. A keyword outside the vocabulary leaves the answer undecided, never satisfied —
 * which matters under {@code not}, where an undecided subschema must not count as one that holds.
 *
 * <p>Values are what a producer holds a default as: {@link Map}, {@link List}, {@link String},
 * {@link Boolean}, {@link Number} and {@code null}. No streams, per the project's convention.
 */
public final class ConstraintEvaluator {

    /** Deeper than this and the evaluator stops deciding, rather than risking the stack. */
    private static final int MAX_DEPTH = 32;

    private static final Set<String> ANNOTATIONS = Set.of("description", "title", "default", "examples", "$comment");

    /** What evaluating one value against one schema established. */
    public sealed interface Verdict permits Holds, Fails, Undecided {}

    /** Every keyword was evaluated and none failed. */
    public record Holds() implements Verdict {}

    /**
     * A keyword failed.
     *
     * @param reason where and how, as a sentence fragment
     */
    public record Fails(String reason) implements Verdict {}

    /** Nothing failed, and something could not be evaluated. */
    public record Undecided() implements Verdict {}

    private static final Verdict HOLDS = new Holds();
    private static final Verdict UNDECIDED = new Undecided();

    private ConstraintEvaluator() {}

    /**
     * {@code value} against {@code constraint}.
     *
     * @param constraint a key's published constraint
     * @param value      the value to hold to it
     */
    public static Verdict verdict(final Map<String, Object> constraint, @Nullable final Object value) {
        return evaluate(constraint, value, "", 0);
    }

    private static Verdict and(final Verdict left, final Verdict right) {
        if (left instanceof Fails) {
            return left;
        }
        if (right instanceof Fails) {
            return right;
        }
        if (left instanceof Undecided || right instanceof Undecided) {
            return UNDECIDED;
        }
        return HOLDS;
    }

    private static Verdict evaluate(
            @Nullable final Object schemaValue, @Nullable final Object value, final String at, final int depth) {
        if (depth > MAX_DEPTH || !(schemaValue instanceof Map<?, ?> schema)) {
            return UNDECIDED;
        }
        // CHECKSTYLE.OFF: Indentation -- palantirJavaFormat wraps each arrow-case body at 4 spaces
        // past `case`; the fetched ruleset's Indentation check wants 8. Reformatting by hand would
        // just be undone by the next spotlessApply, so this scoped disable defers to the formatter
        // that actually governs this file (see terrace-config.java-conventions.gradle.kts'
        // checkstyle block).
        Verdict result = HOLDS;
        for (final Map.Entry<?, ?> entry : schema.entrySet()) {
            final String keyword = String.valueOf(entry.getKey());
            final Object argument = entry.getValue();
            final Verdict found =
                    switch (keyword) {
                        case "type" -> ofType(argument, value, at);
                        case "enum" -> oneOf(argument, value, at);
                        case "const" ->
                            same(argument, value)
                                    ? HOLDS
                                    : fails(at, "is " + show(value) + ", which is not " + show(argument));
                        case "not" -> negated(argument, value, at, depth);
                        case "minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum" ->
                            bound(keyword, argument, value, at);
                        case "minLength", "maxLength" -> length(keyword, argument, value, at);
                        case "pattern" -> pattern(argument, value, at);
                        case "minItems", "maxItems" -> count(keyword, argument, value, at);
                        case "uniqueItems" -> unique(argument, value, at);
                        case "items" -> items(argument, value, at, depth);
                        case "required" -> required(argument, value, at);
                        case "properties" -> properties(argument, value, at, depth);
                        case "additionalProperties" -> additional(schema, argument, value, at, depth);
                        case "propertyNames" -> names(argument, value, at, depth);
                        case "oneOf" -> oneOfBranches(argument, value, at, depth);
                        case "if" -> conditional(schema, argument, value, at, depth);
                        // Read by `if`, whose consequence they are; on their own they assert nothing.
                        case "then", "else" -> HOLDS;
                        case "minProperties", "maxProperties" -> size(keyword, argument, value, at);
                        case "allOf" -> allOf(argument, value, at, depth);
                        case "anyOf" -> anyOf(argument, value, at, depth);
                        default -> ANNOTATIONS.contains(keyword) ? HOLDS : UNDECIDED;
                    };
            result = and(result, found);
            if (result instanceof Fails) {
                return result;
            }
        }
        return result;
        // CHECKSTYLE.ON: Indentation
    }

    private static Verdict fails(final String at, final String message) {
        return new Fails(at.isEmpty() ? message : "`" + at + "` " + message);
    }

    private static Verdict ofType(@Nullable final Object argument, @Nullable final Object value, final String at) {
        final List<String> names = new ArrayList<>();
        if (argument instanceof String name) {
            names.add(name);
        } else if (argument instanceof List<?> list) {
            for (final Object name : list) {
                if (name instanceof String text) {
                    names.add(text);
                }
            }
        } else {
            return UNDECIDED;
        }
        for (final String name : names) {
            if (isType(value, name)) {
                return HOLDS;
            }
        }
        return fails(at, "is " + show(value) + ", which is not of type " + show(argument));
    }

    /** JSON Schema's types, with any number of no fractional part an integer. */
    private static boolean isType(@Nullable final Object value, final String name) {
        return switch (name) {
            case "null" -> value == null;
            case "boolean" -> value instanceof Boolean;
            case "object" -> value instanceof Map<?, ?>;
            case "array" -> value instanceof List<?>;
            case "string" -> value instanceof String;
            case "number" -> value instanceof Number;
            case "integer" -> {
                final BigDecimal number = decimal(value);
                yield number != null && number.stripTrailingZeros().scale() <= 0;
            }
            default -> false;
        };
    }

    private static Verdict oneOf(@Nullable final Object argument, @Nullable final Object value, final String at) {
        if (!(argument instanceof List<?> choices)) {
            return UNDECIDED;
        }
        for (final Object choice : choices) {
            if (same(choice, value)) {
                return HOLDS;
            }
        }
        return fails(at, "is " + show(value) + ", which is not one of " + show(argument));
    }

    /** JSON Schema equality, under which {@code 1} and {@code 1.0} are the same number. */
    private static boolean same(@Nullable final Object left, @Nullable final Object right) {
        final BigDecimal leftNumber = decimal(left);
        final BigDecimal rightNumber = decimal(right);
        if (leftNumber != null && rightNumber != null) {
            return leftNumber.compareTo(rightNumber) == 0;
        }
        if (left instanceof List<?> leftItems && right instanceof List<?> rightItems) {
            return sameItems(leftItems, rightItems);
        }
        if (left instanceof Map<?, ?> leftFields && right instanceof Map<?, ?> rightFields) {
            return sameFields(leftFields, rightFields);
        }
        return left == null ? right == null : left.equals(right);
    }

    private static boolean sameItems(final List<?> left, final List<?> right) {
        if (left.size() != right.size()) {
            return false;
        }
        for (int i = 0; i < left.size(); i++) {
            if (!same(left.get(i), right.get(i))) {
                return false;
            }
        }
        return true;
    }

    private static boolean sameFields(final Map<?, ?> left, final Map<?, ?> right) {
        if (left.size() != right.size()) {
            return false;
        }
        for (final Map.Entry<?, ?> field : left.entrySet()) {
            if (!right.containsKey(field.getKey()) || !same(field.getValue(), right.get(field.getKey()))) {
                return false;
            }
        }
        return true;
    }

    private static Verdict negated(
            @Nullable final Object argument, @Nullable final Object value, final String at, final int depth) {
        final Verdict inner = evaluate(argument, value, at, depth + 1);
        if (inner instanceof Holds) {
            return fails(at, "is " + show(value) + ", which the schema excludes");
        }
        // An undecided subschema is not one that holds; reading it as one would refuse on a guess.
        return inner instanceof Fails ? HOLDS : UNDECIDED;
    }

    private static Verdict bound(
            final String keyword, @Nullable final Object argument, @Nullable final Object value, final String at) {
        final BigDecimal number = decimal(value);
        if (number == null) {
            // A bound constrains numbers and lets everything else past; `type` refuses a string.
            return HOLDS;
        }
        final BigDecimal limit = decimal(argument);
        if (limit == null) {
            return UNDECIDED;
        }
        final int ordering = number.compareTo(limit);
        final boolean outside;
        final String phrasing;
        switch (keyword) {
            case "minimum" -> {
                outside = ordering < 0;
                phrasing = "below the minimum";
            }
            case "maximum" -> {
                outside = ordering > 0;
                phrasing = "above the maximum";
            }
            case "exclusiveMinimum" -> {
                outside = ordering <= 0;
                phrasing = "not above the exclusive minimum";
            }
            default -> {
                outside = ordering >= 0;
                phrasing = "not below the exclusive maximum";
            }
        }
        return outside ? fails(at, "is " + show(value) + ", " + phrasing + " " + show(argument)) : HOLDS;
    }

    private static Verdict length(
            final String keyword, @Nullable final Object argument, @Nullable final Object value, final String at) {
        if (!(value instanceof String text)) {
            return HOLDS;
        }
        final BigDecimal limit = decimal(argument);
        if (limit == null) {
            return UNDECIDED;
        }
        final int held = text.codePointCount(0, text.length());
        final int compared = BigDecimal.valueOf(held).compareTo(limit);
        if ("minLength".equals(keyword) && compared < 0) {
            return fails(at, "is " + show(value) + ", shorter than " + show(argument) + " character(s)");
        }
        if ("maxLength".equals(keyword) && compared > 0) {
            return fails(at, "is " + show(value) + ", longer than " + show(argument) + " character(s)");
        }
        return HOLDS;
    }

    /**
     * {@code pattern}, for a pattern inside the portable subset — the only patterns this module can
     * promise every other engine reads as it does. Any other is undecided.
     */
    private static Verdict pattern(@Nullable final Object argument, @Nullable final Object value, final String at) {
        if (!(value instanceof String text)) {
            return HOLDS;
        }
        final Pattern matcher = argument instanceof String source ? PortablePattern.compile(source) : null;
        if (matcher == null) {
            return UNDECIDED;
        }
        return matcher.matcher(text).find()
                ? HOLDS
                : fails(at, "is " + show(value) + ", which does not match " + show(argument));
    }

    private static Verdict count(
            final String keyword, @Nullable final Object argument, @Nullable final Object value, final String at) {
        if (!(value instanceof List<?> items)) {
            return HOLDS;
        }
        final BigDecimal limit = decimal(argument);
        if (limit == null) {
            return UNDECIDED;
        }
        final int compared = BigDecimal.valueOf(items.size()).compareTo(limit);
        if ("minItems".equals(keyword) && compared < 0) {
            return fails(at, "has " + items.size() + " item(s), fewer than " + show(argument));
        }
        if ("maxItems".equals(keyword) && compared > 0) {
            return fails(at, "has " + items.size() + " item(s), more than " + show(argument));
        }
        return HOLDS;
    }

    private static Verdict unique(@Nullable final Object argument, @Nullable final Object value, final String at) {
        if (!(value instanceof List<?> items) || !Boolean.TRUE.equals(argument)) {
            return HOLDS;
        }
        for (int i = 0; i < items.size(); i++) {
            for (int j = 0; j < i; j++) {
                if (same(items.get(j), items.get(i))) {
                    return fails(at, "repeats " + show(items.get(i)) + ", and every item has to be distinct");
                }
            }
        }
        return HOLDS;
    }

    private static Verdict items(
            @Nullable final Object argument, @Nullable final Object value, final String at, final int depth) {
        if (!(value instanceof List<?> held)) {
            return HOLDS;
        }
        if (!(argument instanceof Map<?, ?>)) {
            return UNDECIDED;
        }
        Verdict result = HOLDS;
        for (int i = 0; i < held.size(); i++) {
            result = and(result, evaluate(argument, held.get(i), at + "[" + i + "]", depth + 1));
            if (result instanceof Fails) {
                break;
            }
        }
        return result;
    }

    /** Every missing name at once: naming one of three is a second round trip. */
    private static Verdict required(@Nullable final Object argument, @Nullable final Object value, final String at) {
        if (!(value instanceof Map<?, ?> fields)) {
            return HOLDS;
        }
        if (!(argument instanceof List<?> names)) {
            return UNDECIDED;
        }
        final List<String> missing = new ArrayList<>();
        for (final Object name : names) {
            if (name instanceof String entry && !fields.containsKey(entry)) {
                missing.add("`" + entry + "`");
            }
        }
        if (missing.isEmpty()) {
            return HOLDS;
        }
        if (missing.size() == 1) {
            return fails(at, "is missing the required entry " + missing.get(0));
        }
        return fails(at, "is missing the required entries " + String.join(", ", missing));
    }

    private static Verdict properties(
            @Nullable final Object argument, @Nullable final Object value, final String at, final int depth) {
        if (!(value instanceof Map<?, ?> fields)) {
            return HOLDS;
        }
        if (!(argument instanceof Map<?, ?> schemas)) {
            return UNDECIDED;
        }
        Verdict result = HOLDS;
        for (final Map.Entry<?, ?> property : schemas.entrySet()) {
            if (fields.containsKey(property.getKey())) {
                result = and(
                        result,
                        evaluate(
                                property.getValue(),
                                fields.get(property.getKey()),
                                member(at, property.getKey()),
                                depth + 1));
                if (result instanceof Fails) {
                    break;
                }
            }
        }
        return result;
    }

    private static Verdict additional(
            final Map<?, ?> schema,
            @Nullable final Object argument,
            @Nullable final Object value,
            final String at,
            final int depth) {
        if (!(value instanceof Map<?, ?> fields)) {
            return HOLDS;
        }
        final Object declared = schema.get("properties");
        Verdict result = HOLDS;
        for (final Map.Entry<?, ?> field : fields.entrySet()) {
            if (declared instanceof Map<?, ?> names && names.containsKey(field.getKey())) {
                continue;
            }
            final Verdict found;
            if (Boolean.TRUE.equals(argument)) {
                found = HOLDS;
            } else if (Boolean.FALSE.equals(argument)) {
                found = fails(member(at, field.getKey()), "is not a key this table declares");
            } else if (argument instanceof Map<?, ?>) {
                found = evaluate(argument, field.getValue(), member(at, field.getKey()), depth + 1);
            } else {
                found = UNDECIDED;
            }
            result = and(result, found);
            if (result instanceof Fails) {
                break;
            }
        }
        return result;
    }

    /** {@code propertyNames}: every entry name, as a string, against the schema. */
    private static Verdict names(
            @Nullable final Object argument, @Nullable final Object value, final String at, final int depth) {
        if (!(value instanceof Map<?, ?> fields)) {
            return HOLDS;
        }
        Verdict result = HOLDS;
        for (final Object name : fields.keySet()) {
            Verdict found = evaluate(argument, String.valueOf(name), "", depth + 1);
            if (found instanceof Fails failed) {
                found = fails(at, "has an entry name that " + failed.reason());
            }
            result = and(result, found);
            if (result instanceof Fails) {
                break;
            }
        }
        return result;
    }

    private static Verdict allOf(
            @Nullable final Object argument, @Nullable final Object value, final String at, final int depth) {
        if (!(argument instanceof List<?> schemas)) {
            return UNDECIDED;
        }
        Verdict result = HOLDS;
        for (final Object schema : schemas) {
            Verdict found = evaluate(schema, value, at, depth + 1);
            // A member carrying `description` is a condition a refinement stated, and fails as that
            // sentence: the operator reads the rule, not its encoding.
            if (found instanceof Fails
                    && schema instanceof Map<?, ?> member
                    && member.get("description") instanceof String description) {
                found = fails(at, "does not satisfy: " + description);
            }
            result = and(result, found);
            if (result instanceof Fails) {
                break;
            }
        }
        return result;
    }

    /** Fails only when every branch fails: one undecided branch could be the one that holds. */
    private static Verdict anyOf(
            @Nullable final Object argument, @Nullable final Object value, final String at, final int depth) {
        if (!(argument instanceof List<?> schemas) || schemas.isEmpty()) {
            return UNDECIDED;
        }
        final List<String> reasons = new ArrayList<>();
        for (final Object schema : schemas) {
            final Verdict found = evaluate(schema, value, at, depth + 1);
            if (found instanceof Fails failed) {
                reasons.add(failed.reason());
            } else {
                return found;
            }
        }
        return new Fails(String.join("; and ", reasons));
    }

    /** {@code oneOf}: exactly one branch holds. */
    private static Verdict oneOfBranches(
            @Nullable final Object argument, @Nullable final Object value, final String at, final int depth) {
        if (!(argument instanceof List<?> schemas) || schemas.isEmpty()) {
            return UNDECIDED;
        }
        int holding = 0;
        boolean undecided = false;
        for (final Object schema : schemas) {
            final Verdict found = evaluate(schema, value, at, depth + 1);
            if (found instanceof Holds) {
                holding++;
            } else if (found instanceof Undecided) {
                undecided = true;
            }
        }
        if (holding >= 2) {
            return fails(
                    at, "satisfies " + holding + " of " + schemas.size() + " alternatives, and exactly one must hold");
        }
        if (undecided) {
            return UNDECIDED;
        }
        return holding == 1
                ? HOLDS
                : fails(at, "satisfies none of " + schemas.size() + " alternatives, and exactly one must hold");
    }

    /** {@code if}, with the {@code then} and {@code else} beside it. */
    private static Verdict conditional(
            final Map<?, ?> schema,
            @Nullable final Object argument,
            @Nullable final Object value,
            final String at,
            final int depth) {
        final Verdict condition = evaluate(argument, value, at, depth + 1);
        final Verdict then = schema.containsKey("then") ? evaluate(schema.get("then"), value, at, depth + 1) : HOLDS;
        final Verdict otherwise =
                schema.containsKey("else") ? evaluate(schema.get("else"), value, at, depth + 1) : HOLDS;
        if (condition instanceof Holds) {
            return then;
        }
        if (condition instanceof Fails) {
            return otherwise;
        }
        return then instanceof Holds && otherwise instanceof Holds ? HOLDS : UNDECIDED;
    }

    private static Verdict size(
            final String keyword, @Nullable final Object argument, @Nullable final Object value, final String at) {
        if (!(value instanceof Map<?, ?> fields)) {
            return HOLDS;
        }
        final BigDecimal limit = decimal(argument);
        if (limit == null) {
            return UNDECIDED;
        }
        final int compared = BigDecimal.valueOf(fields.size()).compareTo(limit);
        if ("minProperties".equals(keyword) && compared < 0) {
            return fails(at, "has " + fields.size() + " entr(ies), fewer than " + show(argument));
        }
        if ("maxProperties".equals(keyword) && compared > 0) {
            return fails(at, "has " + fields.size() + " entr(ies), more than " + show(argument));
        }
        return HOLDS;
    }

    private static String member(final String at, final Object name) {
        return at.isEmpty() ? String.valueOf(name) : at + "." + name;
    }

    /** A number as an exact decimal, or {@code null} for anything else, a non-finite float included. */
    private static @Nullable BigDecimal decimal(@Nullable final Object value) {
        if (value instanceof BigDecimal exact) {
            return exact;
        }
        if (value instanceof BigInteger integer) {
            return new BigDecimal(integer);
        }
        if (value instanceof Double || value instanceof Float) {
            final double real = ((Number) value).doubleValue();
            return Double.isFinite(real) ? BigDecimal.valueOf(real) : null;
        }
        if (value instanceof Number number) {
            return BigDecimal.valueOf(number.longValue());
        }
        return null;
    }

    /**
     * A value as a message shows it: strings quoted, everything else as written.
     *
     * @param value the value to show
     */
    public static String show(@Nullable final Object value) {
        if (value instanceof String text) {
            return "\"" + text + "\"";
        }
        if (value instanceof Map<?, ?> fields && fields.isEmpty()) {
            return "{}";
        }
        return String.valueOf(value);
    }
}
