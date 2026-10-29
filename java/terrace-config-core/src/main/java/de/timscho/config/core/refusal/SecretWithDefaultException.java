package de.timscho.config.core.refusal;

/**
 * Refusal 6: a secret carries a default, in either order the two were declared and anywhere in
 * the document. This is the point the document crosses into a public registry.
 */
public final class SecretWithDefaultException extends ContractRefusalException {

    /**
     * Creates the refusal for one key.
     *
     * @param path the secret key carrying a default
     */
    public SecretWithDefaultException(final String path) {
        super("`" + path + "` is a secret and must not carry a default");
    }
}
