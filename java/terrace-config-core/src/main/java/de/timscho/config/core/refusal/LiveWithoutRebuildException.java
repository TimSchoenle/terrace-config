package de.timscho.config.core.refusal;

/**
 * Refusal 10: a key is published {@code reload: live} while {@code schema.reload} is absent or its
 * {@code mode} is not {@code rebuild}. The key claims a rebuild applies it, and the same document
 * says there is no rebuild.
 */
public final class LiveWithoutRebuildException extends ContractRefusalException {

    public LiveWithoutRebuildException(final String path) {
        super("`" + path + "` is published `live`, and schema.reload says the image does not rebuild");
    }
}
