package de.timscho.config.core.refusal;

/**
 * Refusal 9: a key's {@code default_value} fails its own {@code constraint}. A contract cannot say
 * a key may be left out and that what it falls back to is invalid; a consumer generating values
 * from the default would deploy something that fails at boot.
 */
public final class DefaultViolatesConstraintException extends ContractRefusalException {

    private static final long serialVersionUID = 1L;

    public DefaultViolatesConstraintException(final String path, final String defaultValue, final String reason) {
        super("`" + path + "` publishes the default " + defaultValue + ", which fails its own constraint: "
                + (reason.startsWith("`") ? reason : "it " + reason)
                + ". Fix the default or the annotation; a refined key loses its default through Schema#refine.");
    }
}
