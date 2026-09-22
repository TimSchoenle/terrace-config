package de.timscho.config.core.schema;

import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.UnreachableReason;
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
 * spell, unions with what is there, and never loosens. A default the refined constraint rejects
 * makes the key required with no default, and {@link Defaults} applies the same rule, so the two
 * commute.
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
     * @throws RefinementException for an unknown path, a key that is not an open map, or an entry
     *                             name some layer reaching the key could not spell
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
        // CHECKSTYLE.OFF: Indentation -- palantirJavaFormat wraps each arrow-case body at 4 spaces
        // past `case`; the fetched ruleset's Indentation check wants 8. Reformatting by hand would
        // just be undone by the next spotlessApply, so this scoped disable defers to the formatter
        // that actually governs this file (see terrace-config.java-conventions.gradle.kts'
        // checkstyle block).
        final Key refined =
                switch (refinement) {
                    case Refinement.RequiredEntries required ->
                        requireEntries(keys.get(index), schema.getDialect(), required.entries());
                };
        // CHECKSTYLE.ON: Indentation
        keys.set(index, refined);
        return schema.toBuilder().keys(keys).build();
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
        final Map<String, Object> constraint = key.getConstraint();
        final List<String> entries = new ArrayList<>();
        if (constraint == null
                || !"object".equals(constraint.get("type"))
                || constraint.containsKey("properties")
                || !(constraint.get("required") instanceof List<?> names)) {
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
     * Whether an observed default leaves out an entry the key's constraint requires.
     *
     * <p>Only the refinement's own keyword is read. A default failing the constraint for some other
     * reason is a defect in what the type published, which {@link
     * de.timscho.config.core.refusal.ContractValidator} refuses; turning it into a required key
     * here would hide it.
     *
     * @param key   the key whose constraint to read
     * @param value the observed default
     */
    public static boolean lacksRequiredEntries(final Key key, @Nullable final Object value) {
        if (!(value instanceof Map<?, ?> map)) {
            return false;
        }
        for (final String entry : requiredEntries(key)) {
            if (!map.containsKey(entry)) {
                return true;
            }
        }
        return false;
    }

    private static Key requireEntries(final Key key, final Dialect dialect, final SortedSet<String> entries) {
        final Map<String, Object> constraint = mapConstraint(key);
        for (final String entry : entries) {
            checkSpellable(key, dialect, entry);
        }
        if (entries.isEmpty()) {
            return key;
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
        required.addAll(entries);

        final Map<String, Object> refinedConstraint = new TreeMap<>(constraint);
        refinedConstraint.put("required", new ArrayList<>(required));
        final Key refined = key.toBuilder().constraint(refinedConstraint).build();

        if (lacksRequiredEntries(refined, refined.getDefaultValue())) {
            return refined.toBuilder()
                    .required(true)
                    .defaultText(null)
                    .defaultValue(null)
                    .build();
        }
        return refined;
    }

    /** The key's constraint, when it describes a map a required entry can be added to. */
    private static Map<String, Object> mapConstraint(final Key key) {
        final Map<String, Object> constraint = key.getConstraint();
        if (constraint == null) {
            throw new RefinementException("`" + key.getPath() + "` publishes no constraint, so nothing says it is a "
                    + "map and a required entry has nowhere to be stated. Refine a key whose type is a map.");
        }
        final Object element = constraint.get("additionalProperties");
        final boolean open = element == null || element instanceof Map<?, ?> || Boolean.TRUE.equals(element);
        if (!"object".equals(constraint.get("type")) || constraint.containsKey("properties") || !open) {
            throw new RefinementException("`" + key.getPath() + "` is not a map, so it has no entries to require: "
                    + "its constraint is " + constraint + ". A required entry needs an object whose entries are "
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
