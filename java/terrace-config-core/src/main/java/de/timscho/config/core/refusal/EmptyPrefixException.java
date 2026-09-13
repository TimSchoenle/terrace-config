package de.timscho.config.core.refusal;

/**
 * Refusal 7: {@code dialect.prefix} is empty. A prefixless loader cannot tell its own namespace
 * from the machine's, and every gate in {@code spec/v1/FORMAT.md} rests on that distinction.
 */
public final class EmptyPrefixException extends ContractRefusalException {

    public EmptyPrefixException() {
        super("schema.dialect.prefix must not be empty");
    }
}
