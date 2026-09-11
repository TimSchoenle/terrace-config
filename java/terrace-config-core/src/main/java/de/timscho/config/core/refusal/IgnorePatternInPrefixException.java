package de.timscho.config.core.refusal;

/**
 * Refusal 2: an ignore pattern reaches into {@code dialect.prefix}'s namespace — the same
 * exemption as {@link ExternalVariableInPrefixException} through the other door, and worse,
 * because a pattern exempts everything it happens to cover. A pattern that does not carry the
 * prefix but subsumes it (e.g. {@code PORT*} against {@code PORTFOLIO_}) still triggers this.
 */
public final class IgnorePatternInPrefixException extends ContractRefusalException {

    public IgnorePatternInPrefixException(String pattern, String prefix) {
        super("ignore pattern `" + pattern + "` reaches into the dialect prefix `" + prefix + "`");
    }
}
