package de.timscho.config.core.refusal;

/**
 * Refusal 4: an ignore pattern covers a spelling the loader reads. The prefix is not the whole
 * namespace — the config and secrets-directory variables take arbitrary names — so a pattern
 * exempting one of those disables a gate outside the prefix too.
 */
public final class IgnorePatternCollisionException extends ContractRefusalException {

    public IgnorePatternCollisionException(String pattern, String env) {
        super("ignore pattern `" + pattern + "` covers `" + env + "`, which the loader reads");
    }
}
