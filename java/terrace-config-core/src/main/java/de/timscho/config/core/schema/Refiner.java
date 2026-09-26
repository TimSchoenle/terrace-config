package de.timscho.config.core.schema;

import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.UnreachableReason;
import de.timscho.config.core.refusal.ConstraintEvaluator;
import java.util.ArrayList;
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
 * canonical path, applies only to an open map, names only entries every layer reaching the key can
 * spell, holds a pattern to the portable subset, refuses two refinements that contradict each
 * other, unions with what is there, and never loosens. A default the refinement rejects makes the
 * key required with no default, and {@link Defaults} applies the same rule, so the two commute.
 *
 * <p>No streams, per the project's convention.
 */
@UtilityClass
public class Refiner {

    /**
     * {@code schema} with the key at {@code path} tightened by {@code refinement}.
     *
     * @param schema     the schema to refine; not modified
     * @param path       the key's canonical dotted path
     * @param refinement the tightening
     * @throws RefinementException for an unknown path, a key that is not an open map, an entry name
     *                             some layer reaching the key could not spell, a pattern outside
     *                             the portable subset, or two refinements that contradict each
     *                             other
     */
    public static Schema refine(final Schema schema, final String path, final Refinement refinement) {
        final List<Key> keys = new ArrayList<>(schema.getKeys());
        int index = -1;
        for (int i = 0; i < keys.size(); i++) {
            if (keys.get(i).getPath().equals(path)) {
                index = i;
                break;
            }
        }
        if (index < 0) {
            throw new RefinementException(unknownPath(schema, path));
        }
        final Key key = keys.get(index);
        // CHECKSTYLE.OFF: Indentation -- palantirJavaFormat wraps each arrow-case body at 4 spaces
        // past `case`; the fetched ruleset's Indentation check wants 8. Reformatting by hand would
        // just be undone by the next spotlessApply, so this scoped disable defers to the formatter
        // that actually governs this file (see terrace-config.java-conventions.gradle.kts'
        // checkstyle block).
        final Map<String, Object> refinedConstraint =
                switch (refinement) {
                    case Refinement.RequiredEntries required ->
                        requireEntries(key, schema.getDialect(), required.entries());
                    case Refinement.EntryNames names -> nameEntries(key, names.pattern());
                };
        // CHECKSTYLE.ON: Indentation
        if (refinedConstraint == null) {
            return schema;
        }
        final Map<String, Object> stated = key.getStated() != null ? key.getStated() : key.getConstraint();
        final Key refined = forgetRejectedDefault(
                key.toBuilder().constraint(refinedConstraint).stated(stated).build());
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
     * The entries a map-typed key's constraint requires, in the order it publishes them. Empty for
     * anything but an object that declares no {@code properties}.
     *
     * <p>The single source every rendering reads, rather than a field of its own on {@link Key}
     * that could come to disagree with the constraint a validator applies.
     *
     * @param key the key whose constraint to read
     */
    public static List<String> requiredEntries(final Key key) {
        final Map<String, Object> constraint = mapOf(key);
        final List<String> entries = new ArrayList<>();
        if (constraint == null || !(constraint.get("required") instanceof List<?> names)) {
            return entries;
        }
        for (final Object name : names) {
            if (name instanceof String entry) {
                entries.add(entry);
            }
        }
        return entries;
    }

    /**
     * The pattern a map-typed key's constraint holds its entry names to, or {@code null} when it
     * has none. Read from {@code propertyNames}, as {@link #requiredEntries} reads {@code required}.
     *
     * @param key the key whose constraint to read
     */
    public static @Nullable String entryNamePattern(final Key key) {
        final Map<String, Object> constraint = mapOf(key);
        if (constraint != null
                && constraint.get("propertyNames") instanceof Map<?, ?> names
                && names.get("pattern") instanceof String pattern) {
            return pattern;
        }
        return null;
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

    /** The refined constraint, or {@code null} when refining changed nothing. */
    private static @Nullable Map<String, Object> requireEntries(
            final Key key, final Dialect dialect, final SortedSet<String> entries) {
        final Map<String, Object> constraint = mapConstraint(key, "a required entry");
        for (final String entry : entries) {
            checkSpellable(key, dialect, entry);
        }
        if (constraint.get("propertyNames") instanceof Map<?, ?> names) {
            for (final String entry : entries) {
                final String why = refusedName(names, entry);
                if (why != null) {
                    throw new RefinementException("`" + entry + "` cannot be a required entry of `" + key.getPath()
                            + "`: its entry names are held to " + ConstraintEvaluator.show(names) + ", and the name "
                            + why + ". No map could both hold it and satisfy that.");
                }
            }
        }

        final SortedSet<String> required = new TreeSet<>();
        final Object held = constraint.get("required");
        if (held instanceof List<?> names) {
            for (final Object name : names) {
                if (!(name instanceof String entry)) {
                    throw notEntryNames(key, held);
                }
                required.add(entry);
            }
        } else if (held != null) {
            throw notEntryNames(key, held);
        }
        final int before = required.size();
        required.addAll(entries);
        if (required.size() == before) {
            return null;
        }
        final Map<String, Object> refinedConstraint = new TreeMap<>(constraint);
        refinedConstraint.put("required", new ArrayList<>(required));
        return refinedConstraint;
    }

    /** The refined constraint, or {@code null} when refining changed nothing. */
    private static @Nullable Map<String, Object> nameEntries(final Key key, final String pattern) {
        final Map<String, Object> constraint = mapConstraint(key, "an entry-name pattern");
        final String path = key.getPath();
        final String refused = PortablePattern.refusal(pattern);
        if (refused != null) {
            throw new RefinementException("`" + pattern + "` cannot be the entry-name pattern of `" + path + "`: "
                    + refused + ". A published pattern must mean the same thing to every engine that reads the "
                    + "contract; see *Portable patterns* in spec/v1/FORMAT.md.");
        }

        final Map<String, Object> names = new TreeMap<>();
        names.put("pattern", pattern);
        final Object held = constraint.get("propertyNames");
        if (names.equals(held)) {
            return null;
        }
        if (held != null) {
            throw new RefinementException("`" + path + "` already holds its entry names to "
                    + ConstraintEvaluator.show(held) + ", so a second pattern `" + pattern
                    + "` would need both to hold. Publish one pattern that says so.");
        }
        for (final String entry : requiredEntries(key)) {
            final String why = refusedName(names, entry);
            if (why != null) {
                throw new RefinementException("`" + pattern + "` cannot be the entry-name pattern of `" + path + "`: `"
                        + path + "` requires the entry `" + entry + "`, and the name " + why
                        + ". No map could both hold it and satisfy the pattern.");
            }
        }
        final Map<String, Object> refinedConstraint = new TreeMap<>(constraint);
        refinedConstraint.put("propertyNames", names);
        return refinedConstraint;
    }

    /** Why {@code name} fails the entry-name schema {@code names}, when it certainly does. */
    private static @Nullable String refusedName(final Map<?, ?> names, final String name) {
        final Map<String, Object> schema = new TreeMap<>();
        for (final Map.Entry<?, ?> keyword : names.entrySet()) {
            schema.put(String.valueOf(keyword.getKey()), keyword.getValue());
        }
        return ConstraintEvaluator.verdict(schema, name) instanceof ConstraintEvaluator.Fails failed
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

    /** The key's constraint, when it is an object that declares no {@code properties}. */
    private static @Nullable Map<String, Object> mapOf(final Key key) {
        final Map<String, Object> constraint = key.getConstraint();
        if (constraint == null || !"object".equals(constraint.get("type")) || constraint.containsKey("properties")) {
            return null;
        }
        return constraint;
    }

    /** The key's constraint, when it describes a map {@code what} can be stated about. */
    private static Map<String, Object> mapConstraint(final Key key, final String what) {
        final Map<String, Object> constraint = key.getConstraint();
        if (constraint == null) {
            throw new RefinementException("`" + key.getPath() + "` publishes no constraint, so nothing says it is a "
                    + "map and " + what + " has nowhere to be stated. Refine a key whose type is a map.");
        }
        final Object element = constraint.get("additionalProperties");
        final boolean open = element == null || element instanceof Map<?, ?> || Boolean.TRUE.equals(element);
        if (!"object".equals(constraint.get("type")) || constraint.containsKey("properties") || !open) {
            throw new RefinementException("`" + key.getPath() + "` is not a map, so it has no entries for " + what
                    + " to describe: its constraint is " + constraint + ". That needs an object whose entries are "
                    + "open, with any element shape under `additionalProperties`.");
        }
        return constraint;
    }

    /** Refuse an entry name some layer reaching the key could not spell. */
    private static void checkSpellable(final Key key, final Dialect dialect, final String entry) {
        final String path = key.getPath();
        final String refused = "`" + entry + "` cannot be a required entry of `" + path + "`: ";
        if (entry.isEmpty()) {
            throw new RefinementException(refused + "an empty name is not an entry any layer can supply.");
        }
        if (entry.contains(".")) {
            throw new RefinementException(refused + "the loader reads `.` as nesting, so `" + path + "." + entry
                    + "` names a table inside the map rather than one entry of it.");
        }

        final String entryPath = path + "." + entry;
        if (key.getEnv() != null && !key.isReserved()) {
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
        if (key.getSecretsFile() != null && Spellings.secretsFileName(dialect, entryPath) == null) {
            throw new RefinementException(refused + "no file in the secrets directory can be named for `" + entryPath
                    + "`, which `" + path + "` itself can be supplied from.");
        }
    }

    private static RefinementException notEntryNames(final Key key, final Object held) {
        return new RefinementException("`" + key.getPath() + "` already carries `required: " + held
                + "`, which is not a list of entry names, so there is nothing sound to union a refinement with.");
    }

    private static String unknownPath(final Schema schema, final String path) {
        final StringBuilder message = new StringBuilder("`")
                .append(path)
                .append("` is not a key in this schema, so a refinement of it would be published nowhere while "
                        + "reporting success.");
        for (final Key key : schema.getKeys()) {
            if (key.getAliases().contains(path)) {
                return message.append(" It is an alias of `")
                        .append(key.getPath())
                        .append("`; refine the canonical path.")
                        .toString();
            }
        }
        for (final Key key : schema.getKeys()) {
            if (key.getPath().startsWith(path + ".")) {
                return message.append(" It is a table; name the key inside it.").toString();
            }
        }
        return message.toString();
    }
}
