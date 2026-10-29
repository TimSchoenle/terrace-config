package de.timscho.config.core.refusal;

/**
 * Refusal 2: an ignore pattern reaches into {@code dialect.prefix}'s namespace. It is the same
 * exemption as {@link ExternalVariableInPrefixException} through the other door, and worse, because
 * a pattern exempts everything it happens to cover. A pattern that does not carry the prefix but
 * subsumes it (e.g. {@code PORT*} against {@code PORTFOLIO_}) still triggers this.
 */
public final class IgnorePatternInPrefixException extends ContractRefusalException {

    /**
     * Creates the refusal for one pattern.
     *
     * @param pattern the ignore pattern
     * @param prefix the dialect prefix it reaches into
     */
    public IgnorePatternInPrefixException(final String pattern, final String prefix) {
        super("ignore pattern `" + pattern + "` reaches into the dialect prefix `" + prefix + "`");
    }
}
