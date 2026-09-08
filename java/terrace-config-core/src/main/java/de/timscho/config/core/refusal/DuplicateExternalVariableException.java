package de.timscho.config.core.refusal;

/**
 * Refusal 5: an external variable is declared twice. Refusing to build beats picking one of two
 * descriptions.
 */
public final class DuplicateExternalVariableException extends ContractRefusalException {

    public DuplicateExternalVariableException(String name) {
        super("external variable `" + name + "` is declared more than once");
    }
}
