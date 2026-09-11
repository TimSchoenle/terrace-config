package de.timscho.config.core.refusal;

/**
 * Refusal 3: an external variable collides with a spelling the loader reads, reached through a
 * reserved name.
 */
public final class ExternalVariableCollisionException extends ContractRefusalException {

    public ExternalVariableCollisionException(String name) {
        super("external variable `" + name + "` collides with a variable the loader reads");
    }
}
