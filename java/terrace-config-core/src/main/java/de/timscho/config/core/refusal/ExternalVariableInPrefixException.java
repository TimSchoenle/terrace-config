package de.timscho.config.core.refusal;

/**
 * Refusal 1: an external variable carries {@code dialect.prefix} — everything in that namespace
 * is a configuration key, and declaring one external would leave it governed and exempt at once.
 */
public final class ExternalVariableInPrefixException extends ContractRefusalException {

    public ExternalVariableInPrefixException(String name, String prefix) {
        super("external variable `" + name + "` carries the dialect prefix `" + prefix + "`");
    }
}
