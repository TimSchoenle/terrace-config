package de.timscho.config.core.refusal;

/**
 * Refusal 5: an external variable is declared twice. Refusing to build beats picking one of two
 * descriptions.
 */
public final class DuplicateExternalVariableException extends ContractRefusalException {

    /**
     * Creates the refusal for one variable.
     *
     * @param name the variable declared more than once
     */
    public DuplicateExternalVariableException(final String name) {
        super("external variable `" + name + "` is declared more than once");
    }
}
