package de.timscho.config.core.refusal;

/**
 * Refusal 12: {@code schema.reload} declares {@code mode: rebuild} with an empty {@code layers} —
 * {@code none}, spelled so that it looks like support.
 */
public final class RebuildWithoutLayersException extends ContractRefusalException {

    public RebuildWithoutLayersException() {
        super("schema.reload declares a rebuild and watches no layer");
    }
}
