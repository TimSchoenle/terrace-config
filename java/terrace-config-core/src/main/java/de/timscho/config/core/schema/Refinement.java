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
 * taught to check. The set grows — an entry-name pattern is the obvious next member — so a caller
 * must not rely on a {@code switch} over it being exhaustive.
 */
public sealed interface Refinement permits Refinement.RequiredEntries {

    /**
     * {@link RequiredEntries} from any collection of names.
     *
     * @param entries the entry names the map must contain
     */
    static Refinement requiredEntries(final Collection<String> entries) {
        return new RequiredEntries(new TreeSet<>(entries));
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
}
