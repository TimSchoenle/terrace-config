package de.timscho.config.core.refusal;

/**
 * A producer MUST fail rather than emit a document containing one of the eight things
 * {@code spec/v1/FORMAT.md}'s "What a producer MUST refuse" lists. Each way a contract could
 * quietly stop being one gets its own subclass, thrown by {@link ContractValidator#validate}.
 */
public abstract class ContractRefusalException extends RuntimeException {

    protected ContractRefusalException(String message) {
        super(message);
    }
}
