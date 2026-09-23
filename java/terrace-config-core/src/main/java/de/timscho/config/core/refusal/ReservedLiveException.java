package de.timscho.config.core.refusal;

/**
 * Refusal 11: a {@code reserved} key is published {@code reload: live}. It is read before the
 * layers exist, from an environment that cannot change for the life of the process.
 */
public final class ReservedLiveException extends ContractRefusalException {

    public ReservedLiveException(final String path) {
        super("`" + path + "` is reserved and must not be published `live`");
    }
}
