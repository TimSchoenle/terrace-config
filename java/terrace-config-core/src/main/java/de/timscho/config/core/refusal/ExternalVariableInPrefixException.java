package de.timscho.config.core.refusal;

/**
 * Refusal 1: an external variable carries {@code dialect.prefix}. Everything in that namespace is a
 * configuration key, and declaring one external would leave it governed and exempt at once.
 */
public final class ExternalVariableInPrefixException extends ContractRefusalException {

    /**
     * Creates the refusal for one variable.
     *
     * @param name the external variable carrying the prefix
     * @param prefix the dialect prefix it carries
     */
    public ExternalVariableInPrefixException(final String name, final String prefix) {
        super("external variable `" + name + "` carries the dialect prefix `" + prefix + "`");
    }
}
