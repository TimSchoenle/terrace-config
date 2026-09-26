package de.timscho.config.core.schema;

import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.UnreachableReason;
import de.timscho.config.core.refusal.ConstraintEvaluator;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.SortedSet;
import java.util.TreeMap;
import java.util.TreeSet;
import lombok.experimental.UtilityClass;
import org.jspecify.annotations.Nullable;

/**
 * Applies a {@link Refinement} to a {@link Schema} — the port of the Rust crate's {@code
 * Schema::refine} and {@code Schema::refine_with}, wired onto {@link Schema#refine} and {@link
 * Schema#refineWith}.
 *
 * <p>Same rules, same order of checks, same messages in substance: a refinement names a key's
 * canonical path, optionally continued into the key's constraint — {@value #ELEMENT} for the
 * element every entry or item shares, a field name for a struct's field — and applies only to a
 * position of the right shape. It names only entries every layer reaching the key can spell, holds
 * a pattern to the portable subset, refuses two refinements that contradict each other, unions with
 * what is there, and never loosens. A default the refinement rejects makes the key required with no
 * default, and {@link Defaults} applies the same rule, so the two commute.
 *
 * <p>No streams, per the project's convention.
 */
@UtilityClass
public class Refiner {

    /** The path segment that addresses every element of a map or a sequence. */
    public static final String ELEMENT = "*";

    /** The segment standing in for an element when an entry name below one is spelled. */
    private static final String SPELLED_ELEMENT = "entry";

    /** How deep {@link #tightenings} follows a constraint. */
    private static final int MAX_DEPTH = 32;

    /** One tightening a constraint publishes, and its position relative to the key. */
    public sealed interface Tightening permits Entries, Names, Matches, Holds {
        /**
         * Where the tightening is: empty for the key itself, {@code *.body} for the {@code body}
         * field of every element.
         *
         * @return the position
         */
        String at();
    }

    /**
     * A map's {@code required} entries.
     *
     * @param at      the position
     * @param entries the entries, as published
     */
    public record Entries(String at, List<String> entries) implements Tightening {}

    /**
     * A map's {@code propertyNames} pattern.
     *
     * @param at      the position
     * @param pattern the pattern
     */
    public record Names(String at, String pattern) implements Tightening {}

    /**
     * A string's {@code pattern}.
     *
     * @param at      the position
     * @param pattern the pattern
     */
    public record Matches(String at, String pattern) implements Tightening {}

    /**
     * A struct's condition, by the sentence its {@code allOf} member carries as {@code description}.
     *
     * @param at          the position
     * @param description the sentence
     */
    public record Holds(String at, String description) implements Tightening {}

    /**
     * {@code schema} with the constraint at {@code path} tightened by {@code refinement}.
     *
     * @param schema     the schema to refine; not modified
     * @param path       a key's canonical dotted path, optionally continued into its constraint
     * @param refinement the tightening
     * @throws RefinementException for an unknown path, a position the type did not describe, a
     *                             position of the wrong shape, an entry name some layer could not
     *                             spell, a pattern outside the portable subset, or two refinements
     *                             that contradict each other
     */
    public static Schema refine(final Schema schema, final String path, final Refinement refinement) {
        final List<Key> keys = new ArrayList<>(schema.getKeys());
        final int index = locate(schema, path);
        final Key key = keys.get(index);
        final List<String> inside = inside(key, path);
        final Reach reach = Reach.of(key, inside);

        if (key.getConstraint() == null) {
            throw new RefinementException("`" + key.getPath() + "` publishes no constraint, so nothing says what it "
                    + "holds and " + what(refinement) + " has nowhere to be stated. Refine a key whose type this "
                    + "module reads.");
        }
        final Map<String, Object> root = copy(key.getConstraint());
        final Map<String, Object> target = descend(root, key.getPath(), inside);
        // CHECKSTYLE.OFF: Indentation -- palantirJavaFormat wraps each arrow-case body at 4 spaces
        // past `case`; the fetched ruleset's Indentation check wants 8. Reformatting by hand would
        // just be undone by the next spotlessApply, so this scoped disable defers to the formatter
        // that actually governs this file (see terrace-config.java-conventions.gradle.kts'
        // checkstyle block).
        final boolean changed =
                switch (refinement) {
                    case Refinement.RequiredEntries required ->
                        requireEntries(target, reach, schema.getDialect(), required.entries());
                    case Refinement.EntryNames names -> nameEntries(target, reach.at(), names.pattern());
                    case Refinement.Pattern matching -> matchPattern(target, reach.at(), matching.pattern());
                    case Refinement.Holds holds -> hold(target, reach.at(), holds.condition());
                };
        // CHECKSTYLE.ON: Indentation
        if (!changed) {
            return schema;
        }
        final Map<String, Object> stated = key.getStated() != null ? key.getStated() : key.getConstraint();
        final Key refined = forgetRejectedDefault(
                key.toBuilder().constraint(root).stated(stated).build());
        keys.set(index, refined);
        return schema.toBuilder()
                .keys(keys)
                .schemaVersion(Math.max(schema.getSchemaVersion(), refinement.schemaVersion()))
                .build();
    }

    /**
     * {@code schema} with every refinement {@code source} publishes applied, its paths read
     * relative to {@code at}.
     *
     * @param schema the schema to refine; not modified
     * @param at     where the source's configuration is mounted; empty for the root
     * @param source the refinements
     * @throws RefinementException as {@link #refine}, for the first refinement that fails, with the
     *                             full path in the message
     */
    public static Schema refineWith(final Schema schema, final String at, final Refine source) {
        Schema current = schema;
        for (final Refine.At refinement : source.refinements()) {
            final String relative = refinement.path();
            final String path;
            if (at.isEmpty()) {
                path = relative;
            } else if (relative.isEmpty()) {
                path = at;
            } else {
                path = at + "." + relative;
            }
            current = refine(current, path, refinement.refinement());
        }
        return current;
    }

    /**
     * Every tightening a key's constraint publishes, in the order a reader meets them: the key's own
     * — entries, entry names, pattern — then its element, then its fields as the constraint lists
     * them. The single source every rendering reads.
     *
     * <p>A struct's {@code required} names its fields rather than a map's entries and is not one;
     * neither is a {@code propertyNames} other than a lone pattern.
     *
     * @param key the key whose constraint to read
     */
    public static List<Tightening> tightenings(final Key key) {
        final List<Tightening> found = new ArrayList<>();
        if (key.getConstraint() != null) {
            walk(key.getConstraint(), "", 0, found);
        }
        return found;
    }

    /**
     * Whether an observed default is one the key's <em>refinements</em> reject.
     *
     * <p>Only a failure the refinements introduced counts: the default satisfies the constraint the
     * type stated and fails the refined one. A default failing for some other reason is a defect in
     * what the type published, which {@link de.timscho.config.core.refusal.ContractValidator}
     * refuses; turning it into a required key here would hide it.
     *
     * @param key   the key whose constraint to read
     * @param value the observed default
     */
    public static boolean rejectedByRefinement(final Key key, @Nullable final Object value) {
        final Map<String, Object> refined = key.getConstraint();
        final Map<String, Object> stated = key.getStated();
        if (refined == null || stated == null) {
            return false;
        }
        return ConstraintEvaluator.verdict(refined, value) instanceof ConstraintEvaluator.Fails
                && !(ConstraintEvaluator.verdict(stated, value) instanceof ConstraintEvaluator.Fails);
    }

    /** Where a refinement lands, and which layers reach it. */
    private record Reach(String at, String spelled, boolean env, boolean secretsFile) {

        static Reach of(final Key key, final List<String> inside) {
            final StringBuilder at = new StringBuilder(key.getPath());
            final StringBuilder spelled = new StringBuilder(key.getPath());
            for (final String segment : inside) {
                at.append('.').append(segment);
                spelled.append('.').append(ELEMENT.equals(segment) ? SPELLED_ELEMENT : segment);
            }
            return new Reach(
                    at.toString(),
                    spelled.toString(),
                    key.getEnv() != null && !key.isReserved(),
                    key.getSecretsFile() != null);
        }
    }

    private static String what(final Refinement refinement) {
        if (refinement instanceof Refinement.RequiredEntries) {
            return "a required entry";
        }
        if (refinement instanceof Refinement.Holds) {
            return "a condition";
        }
        return refinement instanceof Refinement.EntryNames ? "an entry-name pattern" : "a pattern";
    }

    /** The index of the key {@code path} names: its own path, or the longest key path it continues. */
    private static int locate(final Schema schema, final String path) {
        final List<Key> keys = schema.getKeys();
        int best = -1;
        for (int i = 0; i < keys.size(); i++) {
            final String keyPath = keys.get(i).getPath();
            if (keyPath.equals(path)) {
                return i;
            }
            if (path.startsWith(keyPath + ".")
                    && (best < 0 || keyPath.length() > keys.get(best).getPath().length())) {
                best = i;
            }
        }
        if (best < 0) {
            throw new RefinementException(unknownPath(schema, path));
        }
        return best;
    }

    /** The segments of {@code path} that continue inside {@code key}'s constraint. */
    private static List<String> inside(final Key key, final String path) {
        if (path.equals(key.getPath())) {
            return List.of();
        }
        final List<String> segments =
                Arrays.asList(path.substring(key.getPath().length() + 1).split("\\.", -1));
        for (final String segment : segments) {
            if (segment.isEmpty()) {
                throw new RefinementException(
                        "`" + path + "` has an empty segment, so it names no position inside `" + key.getPath() + "`.");
            }
        }
        return segments;
    }

    /** The position {@code inside} names within a copied constraint, or why there is none. */
    private static Map<String, Object> descend(
            final Map<String, Object> root, final String keyPath, final List<String> inside) {
        final StringBuilder at = new StringBuilder(keyPath);
        Map<String, Object> schema = root;
        for (final String segment : inside) {
            schema = ELEMENT.equals(segment) ? element(schema, at.toString()) : field(schema, at.toString(), segment);
            at.append('.').append(segment);
        }
        return schema;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> element(final Map<String, Object> schema, final String at) {
        final String keyword;
        if (isMap(schema)) {
            keyword = "additionalProperties";
        } else if ("array".equals(schema.get("type"))) {
            keyword = "items";
        } else {
            throw new RefinementException("`" + at + "` is neither a map nor a sequence, so it has no element for `"
                    + ELEMENT + "` to address: its constraint is " + schema + ".");
        }
        if (schema.get(keyword) instanceof Map<?, ?> element) {
            return (Map<String, Object>) element;
        }
        throw new RefinementException("`" + at + "` publishes no element schema, so `" + at + "." + ELEMENT
                + "` names nothing a refinement could be stated on. Describe the element, or refine `" + at
                + "` itself.");
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> field(final Map<String, Object> schema, final String at, final String name) {
        if (schema.get("properties") instanceof Map<?, ?> fields && fields.containsKey(name)) {
            if (fields.get(name) instanceof Map<?, ?> field) {
                return (Map<String, Object>) field;
            }
            throw new RefinementException(
                    "`" + at + "." + name + "` publishes no schema a refinement could be stated on.");
        }
        if (!"object".equals(schema.get("type"))) {
            throw new RefinementException("`" + at + "` is not a struct, so it has no field `" + name
                    + "`: its constraint is " + schema + ".");
        }
        if (isMap(schema)) {
            throw new RefinementException("`" + at + "` is a map, and `" + name + "` would be one entry of it. An "
                    + "entry's name is the operator's to choose, so it is no position of the schema; address every "
                    + "entry with `" + at + "." + ELEMENT + "`.");
        }
        final StringBuilder declared = new StringBuilder();
        if (schema.get("properties") instanceof Map<?, ?> fields) {
            for (final Object field : fields.keySet()) {
                declared.append(declared.isEmpty() ? "" : ", ")
                        .append('`')
                        .append(field)
                        .append('`');
            }
        }
        throw new RefinementException("`" + name + "` is not a field of `" + at + "`, so `" + at + "." + name
                + "` names nothing."
                + (declared.isEmpty() ? " It declares no fields." : " Its fields are " + declared + "."));
    }

    private static boolean isMap(final Map<?, ?> schema) {
        return "object".equals(schema.get("type")) && !schema.containsKey("properties");
    }

    /** {@link Refinement.RequiredEntries} at one position. Whether the constraint changed. */
    private static boolean requireEntries(
            final Map<String, Object> target,
            final Reach reach,
            final Dialect dialect,
            final SortedSet<String> entries) {
        final String at = reach.at();
        checkMap(target, at, "a required entry");
        for (final String entry : entries) {
            checkSpellable(reach, dialect, entry);
        }
        if (target.get("propertyNames") instanceof Map<?, ?> names) {
            for (final String entry : entries) {
                final String why = refused(names, entry);
                if (why != null) {
                    throw new RefinementException("`" + entry + "` cannot be a required entry of `" + at
                            + "`: its entry names are held to " + ConstraintEvaluator.show(names) + ", and the name "
                            + why + ". No map could both hold it and satisfy that.");
                }
            }
        }

        final SortedSet<String> required = new TreeSet<>();
        final Object held = target.get("required");
        if (held instanceof List<?> names) {
            for (final Object name : names) {
                if (!(name instanceof String entry)) {
                    throw notEntryNames(at, held);
                }
                required.add(entry);
            }
        } else if (held != null) {
            throw notEntryNames(at, held);
        }
        final int before = required.size();
        required.addAll(entries);
        if (required.size() == before) {
            return false;
        }
        target.put("required", new ArrayList<>(required));
        return true;
    }

    /** {@link Refinement.EntryNames} at one position. Whether the constraint changed. */
    private static boolean nameEntries(final Map<String, Object> target, final String at, final String pattern) {
        checkMap(target, at, "an entry-name pattern");
        checkPortable(pattern, "entry-name pattern", at);

        final Map<String, Object> names = new TreeMap<>();
        names.put("pattern", pattern);
        final Object held = target.get("propertyNames");
        if (names.equals(held)) {
            return false;
        }
        if (held != null) {
            throw new RefinementException("`" + at + "` already holds its entry names to "
                    + ConstraintEvaluator.show(held) + ", so a second pattern `" + pattern
                    + "` would need both to hold. Publish one pattern that says so.");
        }
        if (target.get("required") instanceof List<?> required) {
            for (final Object entry : required) {
                final String why = entry instanceof String name ? refused(names, name) : null;
                if (why != null) {
                    throw new RefinementException("`" + pattern + "` cannot be the entry-name pattern of `" + at
                            + "`: `" + at + "` requires the entry `" + entry + "`, and the name " + why
                            + ". No map could both hold it and satisfy the pattern.");
                }
            }
        }
        target.put("propertyNames", names);
        return true;
    }

    /** {@link Refinement.Pattern} at one position. Whether the constraint changed. */
    private static boolean matchPattern(final Map<String, Object> target, final String at, final String pattern) {
        if (!isString(target.get("type"))) {
            throw new RefinementException("`" + at + "` is not a string, so it has no text for a pattern to match: "
                    + "its constraint is " + target + ".");
        }
        checkPortable(pattern, "pattern", at);

        final Object held = target.get("pattern");
        if (pattern.equals(held)) {
            return false;
        }
        if (held != null) {
            throw new RefinementException("`" + at + "` is already matched against " + ConstraintEvaluator.show(held)
                    + ", so a second pattern `" + pattern + "` would need both to hold. Publish one pattern that "
                    + "says so.");
        }
        checkSomeChoiceMatches(target, at, pattern);
        target.put("pattern", pattern);
        return true;
    }

    private static boolean isString(@Nullable final Object type) {
        return "string".equals(type) || (type instanceof List<?> names && names.contains("string"));
    }

    /** Refuse a pattern no value of a choice matches, which no value could satisfy. */
    private static void checkSomeChoiceMatches(
            final Map<String, Object> target, final String at, final String pattern) {
        if (!(target.get("enum") instanceof List<?> choices) || choices.isEmpty()) {
            return;
        }
        final Map<String, Object> matching = new TreeMap<>();
        matching.put("pattern", pattern);
        for (final Object choice : choices) {
            if (!(choice instanceof String text) || refused(matching, text) == null) {
                return;
            }
        }
        throw new RefinementException("`" + pattern + "` cannot be the pattern of `" + at + "`: `" + at + "` is one of "
                + choices + ", and the pattern matches none of them. No value could satisfy both.");
    }

    /** {@link Refinement.Holds} at one position. Whether the constraint changed. */
    @SuppressWarnings("unchecked")
    private static boolean hold(final Map<String, Object> target, final String at, final Condition condition) {
        if (!"object".equals(target.get("type")) || !(target.get("properties") instanceof Map<?, ?>)) {
            throw new RefinementException("`" + at + "` is not a struct, so it has no fields for a condition to "
                    + "relate: its constraint is " + target + ". State a condition on a struct — the element of a "
                    + "map or sequence of them is `" + at + "." + ELEMENT + "`.");
        }
        final Conditions.Stated stated;
        try {
            stated = Conditions.state(condition, target, at);
        } catch (final Conditions.Refused refused) {
            throw new RefinementException(
                    "the condition cannot be stated on `" + at + "`: " + refused.getMessage() + ".");
        }
        final Map<String, Object> member = new TreeMap<>(stated.schema());
        member.put("description", stated.description());

        final Object held = target.get("allOf");
        final List<Object> conditions;
        if (held == null) {
            conditions = new ArrayList<>();
        } else if (held instanceof List<?> list) {
            conditions = (List<Object>) list;
        } else {
            throw new RefinementException("`" + at + "` already carries `allOf: " + held
                    + "`, which is not a list of schemas, so there is nothing sound to add a condition to.");
        }
        if (conditions.contains(member)) {
            return false;
        }
        conditions.add(member);
        target.put("allOf", conditions);
        return true;
    }

    private static void checkPortable(final String pattern, final String what, final String at) {
        final String refused = PortablePattern.refusal(pattern);
        if (refused != null) {
            throw new RefinementException("`" + pattern + "` cannot be the " + what + " of `" + at + "`: " + refused
                    + ". A published pattern must mean the same thing to every engine that reads the contract; see "
                    + "*Portable patterns* in spec/v1/FORMAT.md.");
        }
    }

    /** Why {@code text} fails {@code schema}, when it certainly does. */
    private static @Nullable String refused(final Map<?, ?> schema, final String text) {
        final Map<String, Object> copied = new TreeMap<>();
        for (final Map.Entry<?, ?> keyword : schema.entrySet()) {
            copied.put(String.valueOf(keyword.getKey()), keyword.getValue());
        }
        return ConstraintEvaluator.verdict(copied, text) instanceof ConstraintEvaluator.Fails failed
                ? failed.reason()
                : null;
    }

    /** Drop a default the refinement just made invalid, making the key required. */
    private static Key forgetRejectedDefault(final Key key) {
        if (key.getDefaultValue() != null && rejectedByRefinement(key, key.getDefaultValue())) {
            return key.toBuilder()
                    .required(true)
                    .defaultText(null)
                    .defaultValue(null)
                    .build();
        }
        return key;
    }

    /** Refuse a schema at {@code at} that is not a map {@code what} can be stated about. */
    private static void checkMap(final Map<String, Object> schema, final String at, final String what) {
        final Object element = schema.get("additionalProperties");
        final boolean open = element == null || element instanceof Map<?, ?> || Boolean.TRUE.equals(element);
        if (!isMap(schema) || !open) {
            throw new RefinementException("`" + at + "` is not a map, so it has no entries for " + what
                    + " to describe: its constraint is " + schema + ". That needs an object whose entries are "
                    + "open, with any element shape under `additionalProperties`.");
        }
    }

    /** Refuse an entry name some layer reaching the map could not spell. */
    private static void checkSpellable(final Reach reach, final Dialect dialect, final String entry) {
        final String at = reach.at();
        final String refused = "`" + entry + "` cannot be a required entry of `" + at + "`: ";
        if (entry.isEmpty()) {
            throw new RefinementException(refused + "an empty name is not an entry any layer can supply.");
        }
        if (entry.contains(".")) {
            throw new RefinementException(refused + "the loader reads `.` as nesting, so `" + at + "." + entry
                    + "` names a table inside the map rather than one entry of it.");
        }

        final String entryPath = reach.spelled() + "." + entry;
        if (reach.env()) {
            final Spellings.Spelling spelling = Spellings.envSpelling(dialect, entryPath);
            if (spelling.getEnv() == null) {
                final String spelled = Spellings.rawEnvSpelling(dialect, entryPath);
                if (spelling.getUnreachable() == UnreachableReason.INDIRECTION) {
                    throw new RefinementException(refused + "`" + spelled + "` is read as the `"
                            + dialect.getIndirectionSuffix()
                            + "` indirection for another name, so no variable supplies this entry.");
                }
                final String arrives = Spellings.envLayerKey(dialect, spelled);
                throw new RefinementException(refused + "`" + spelled
                        + "` is the variable that would supply it, and the environment layer reads that as "
                        + (arrives == null ? "nothing" : "`" + arrives + "`")
                        + ". Name the entry in lower case, without the `" + dialect.getNestingSeparator()
                        + "` separator.");
            }
        }
        if (reach.secretsFile() && Spellings.secretsFileName(dialect, entryPath) == null) {
            throw new RefinementException(refused + "no file in the secrets directory can be named for `" + entryPath
                    + "`, which the key itself can be supplied from.");
        }
    }

    private static RefinementException notEntryNames(final String at, final Object held) {
        return new RefinementException("`" + at + "` already carries `required: " + held
                + "`, which is not a list of entry names, so there is nothing sound to union a refinement with.");
    }

    private static String unknownPath(final Schema schema, final String path) {
        final StringBuilder message = new StringBuilder("`")
                .append(path)
                .append("` is not a key in this schema, so a refinement of it would be published nowhere while "
                        + "reporting success.");
        for (final Key key : schema.getKeys()) {
            for (final String alias : key.getAliases()) {
                if (alias.equals(path) || path.startsWith(alias + ".")) {
                    return message.append(" It is an alias of `")
                            .append(key.getPath())
                            .append("`, or inside one; refine the canonical path.")
                            .toString();
                }
            }
        }
        for (final Key key : schema.getKeys()) {
            if (key.getPath().startsWith(path + ".")) {
                return message.append(" It is a table; name the key inside it.").toString();
            }
        }
        return message.toString();
    }

    private static void walk(final Map<?, ?> schema, final String at, final int depth, final List<Tightening> found) {
        if (depth > MAX_DEPTH) {
            return;
        }
        if (isMap(schema)) {
            mapTightenings(schema, at, found);
        }
        if (schema.get("pattern") instanceof String pattern) {
            found.add(new Matches(at, pattern));
        }
        conditionTightenings(schema, at, found);
        for (final String keyword : List.of("additionalProperties", "items")) {
            if (schema.get(keyword) instanceof Map<?, ?> element) {
                walk(element, below(at, ELEMENT), depth + 1, found);
            }
        }
        if (schema.get("properties") instanceof Map<?, ?> fields) {
            for (final Map.Entry<?, ?> field : new TreeMap<>(fields).entrySet()) {
                if (field.getValue() instanceof Map<?, ?> nested) {
                    walk(nested, below(at, String.valueOf(field.getKey())), depth + 1, found);
                }
            }
        }
    }

    /** A map's own tightenings: its required entries, then its entry-name pattern. */
    private static void mapTightenings(final Map<?, ?> schema, final String at, final List<Tightening> found) {
        if (schema.get("required") instanceof List<?> names && !names.isEmpty()) {
            final List<String> entries = new ArrayList<>();
            for (final Object name : names) {
                if (name instanceof String entry) {
                    entries.add(entry);
                }
            }
            found.add(new Entries(at, entries));
        }
        if (schema.get("propertyNames") instanceof Map<?, ?> names
                && names.size() == 1
                && names.get("pattern") instanceof String pattern) {
            found.add(new Names(at, pattern));
        }
    }

    /** A struct's conditions: the {@code allOf} members carrying {@code description}. */
    private static void conditionTightenings(final Map<?, ?> schema, final String at, final List<Tightening> found) {
        if (!(schema.get("allOf") instanceof List<?> members)) {
            return;
        }
        for (final Object member : members) {
            if (member instanceof Map<?, ?> described && described.get("description") instanceof String said) {
                found.add(new Holds(at, said));
            }
        }
    }

    private static String below(final String at, final String segment) {
        return at.isEmpty() ? segment : at + "." + segment;
    }

    /** A deep, mutable copy of a constraint, so refining never touches the schema it was given. */
    private static Map<String, Object> copy(final Map<?, ?> schema) {
        final Map<String, Object> copied = new TreeMap<>();
        for (final Map.Entry<?, ?> keyword : schema.entrySet()) {
            copied.put(String.valueOf(keyword.getKey()), copyValue(keyword.getValue()));
        }
        return copied;
    }

    private static @Nullable Object copyValue(@Nullable final Object value) {
        if (value instanceof Map<?, ?> nested) {
            return copy(nested);
        }
        if (value instanceof List<?> items) {
            final List<Object> copied = new ArrayList<>(items.size());
            for (final Object item : items) {
                copied.add(copyValue(item));
            }
            return copied;
        }
        return value;
    }
}
