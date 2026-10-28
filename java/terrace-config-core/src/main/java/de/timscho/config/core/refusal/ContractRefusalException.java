package de.timscho.config.core.refusal;

/**
 * A producer MUST fail rather than emit a document containing one of the twelve things
 * {@code spec/v1/FORMAT.md}'s "What a producer MUST refuse" lists. Each way a contract could
 * quietly stop being one gets its own subclass, thrown by {@link ContractValidator#validate}.
 */
public abstract class ContractRefusalException extends RuntimeException {

    /**
     * Creates a refusal carrying its explanation.
     *
     * @param message what the contract would violate, in a form fit to show the author
     */
    protected ContractRefusalException(final String message) {
        super(message);
    }
}
