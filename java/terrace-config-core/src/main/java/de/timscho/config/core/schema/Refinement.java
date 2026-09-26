package de.timscho.config.core.schema;

import java.util.Collection;
import java.util.Collections;
import java.util.SortedSet;
import java.util.TreeSet;

/**
 * One tightening of a key's constraint, beyond what its type states — the port of the Rust crate's
 * {@code schema::Refinement}.
 *
 * <p>Some constraints are real without being in any type: a host refusing to start unless a map of
 * legal documents holds {@code imprint} and {@code privacy}. {@link
 * de.timscho.config.core.model.Schema#refine} publishes one. It is a closed set of typed
 * tightenings rather than a JSON Schema fragment on purpose: a fragment can say anything, including
 * something that widens what the type stated, and nothing downstream could tell.
 *
 * <p>Sealed so that nothing outside this module can add a refinement {@link Refiner} has not been
 * taught to check. The set grows, so a caller must not rely on a {@code switch} over it being
 * exhaustive.
 */
public sealed interface Refinement
        permits Refinement.RequiredEntries, Refinement.EntryNames, Refinement.Pattern, Refinement.Holds {

    /**
     * Text holding at least one character that is not white space — the negation of Rust's {@code
     * str::trim().is_empty()}, exactly. The class is Unicode's {@code White_Space} property. {@code
     * \S} is not a substitute: ECMA-262's {@code \s} holds U+FEFF, which is not white space, so it
     * would reject a value the runtime check accepts.
     */
    String NON_BLANK =
            "[^\\t\\n\\u000B\\f\\r \\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F" + "\\u205F\\u3000]";

    /**
     * {@link Pattern} from a pattern, checked when it is applied.
     *
     * @param pattern the pattern the string must match, inside the portable subset
     */
    static Refinement pattern(final String pattern) {
        return new Pattern(pattern);
    }

    /**
     * {@link Pattern} of {@link #NON_BLANK}: the string is not empty and not only white space.
     *
     * @return the refinement
     */
    static Refinement nonBlank() {
        return new Pattern(NON_BLANK);
    }

    /**
     * {@link RequiredEntries} from any collection of names.
     *
     * @param entries the entry names the map must contain
     */
    static Refinement requiredEntries(final Collection<String> entries) {
        return new RequiredEntries(new TreeSet<>(entries));
    }

    /**
     * {@link EntryNames} from a pattern. Checked when it is applied, where a refusal can name the
     * key it was meant for.
     *
     * @param pattern the pattern every entry name must match, inside the portable subset
     */
    static Refinement entryNames(final String pattern) {
        return new EntryNames(pattern);
    }

    /**
     * {@link Holds} from a condition.
     *
     * @param condition the condition between the struct's fields
     * @return the refinement
     */
    static Refinement holds(final Condition condition) {
        return new Holds(condition);
    }

    /**
     * The version of the schema half a document carrying this refinement is published at.
     *
     * @return {@code 4} for a condition, {@code 3} for an entry-name pattern, {@code 2} otherwise
     */
    default int schemaVersion() {
        if (this instanceof Holds) {
            return 4;
        }
        return this instanceof EntryNames ? 3 : 2;
    }

    /**
     * A map-typed key must contain these entries, whatever else it holds.
     *
     * <p>Published as JSON Schema's {@code required} inside the key's constraint, unioned with any
     * entries already required. The map stays open. An empty set is accepted and changes nothing,
     * once the path and the key's shape have been checked — a library computing its entries at
     * runtime may compute none, and skipping the checks for an empty set would let a mistyped path
     * through exactly then.
     *
     * @param entries the entry names, sorted and de-duplicated
     */
    record RequiredEntries(SortedSet<String> entries) implements Refinement {

        /**
         * Copies {@code entries}, so a caller mutating its set afterwards cannot change what was
         * refined.
         *
         * @param entries the entry names the map must contain
         */
        public RequiredEntries {
            entries = Collections.unmodifiableSortedSet(new TreeSet<>(entries));
        }
    }

    /**
     * Every entry name of a map-typed key must match this pattern.
     *
     * <p>Published as JSON Schema's {@code propertyNames: {"pattern": …}} inside the key's
     * constraint, which puts the document at {@code schema_version: 3}. Matched as JSON Schema
     * matches one — unanchored — and held to the portable subset, {@link PortablePattern}. The
     * environment folds names to lower case, so a pattern admitting an upper-case name describes an
     * entry only a file can supply: weaker than the loader, not wrong, and published as written.
     *
     * @param pattern the pattern every entry name must match
     */
    record EntryNames(String pattern) implements Refinement {}

    /**
     * A string must match this pattern.
     *
     * <p>Published as JSON Schema's {@code pattern} at the string's position — a string-typed key,
     * or a string inside one's element schema. The same portable subset as {@link EntryNames}.
     *
     * @param pattern the pattern the string must match
     */
    record Pattern(String pattern) implements Refinement {}

    /**
     * A condition between the fields of a struct must hold.
     *
     * <p>Stated on a struct position — in practice the element of a map or sequence of structs — and
     * published as one member of that position's {@code allOf}: the condition's JSON Schema, with its
     * readable form as the member's {@code description}. A conjunct, so it only tightens.
     *
     * @param condition the condition
     */
    record Holds(Condition condition) implements Refinement {}
}
