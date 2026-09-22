package de.timscho.config.core.schema;

import java.util.List;

/**
 * A source of refinements — a library whose runtime validation should be published. The port of
 * the Rust crate's {@code schema::Refine}.
 *
 * <p>Implemented by whatever owns the check a type cannot state, so the contract and the check
 * cannot drift. Paths are relative to where the library's configuration is mounted, which only the
 * host knows: {@link de.timscho.config.core.model.Schema#refineWith} takes that mount point, so a
 * library never hard-codes it. An empty relative path is the mount point itself.
 */
@FunctionalInterface
public interface Refine {

    /** Every refinement this source publishes, each with its path relative to the mount. */
    List<At> refinements();

    /**
     * One refinement, at a path relative to wherever its source is mounted.
     *
     * @param path       the dotted path below the mount point; empty for the mount point itself
     * @param refinement the tightening to apply there
     */
    record At(String path, Refinement refinement) {}
}
