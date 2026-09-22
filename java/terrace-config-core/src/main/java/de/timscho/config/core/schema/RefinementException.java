package de.timscho.config.core.schema;

/**
 * A {@link Refinement} that cannot be published: an unknown path, a key that is not a map, or an
 * entry name some layer reaching the key could not spell. The message names the path and says why.
 *
 * <p>Unchecked, as the refusals are: it is a defect in the caller's refinement, found while the
 * contract is built, and there is nothing a caller could do with it but fail the build.
 */
public final class RefinementException extends IllegalArgumentException {

    private static final long serialVersionUID = 1L;

    RefinementException(final String message) {
        super(message);
    }
}
