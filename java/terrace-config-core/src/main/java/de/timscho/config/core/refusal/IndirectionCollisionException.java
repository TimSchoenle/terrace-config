package de.timscho.config.core.refusal;

/**
 * Refusal 8: a key's environment spelling is another key's indirection variable
 * ({@code unreachable: indirection}). With {@code token} and {@code token_file} both present,
 * setting {@code <PREFIX>TOKEN_FILE} fills {@code token} from the file it names <em>and</em>
 * fills {@code token_file} with the path: one variable, two keys, and a validator classifying it
 * stops at the first.
 */
public final class IndirectionCollisionException extends ContractRefusalException {

    public IndirectionCollisionException(String key, String otherKey, String env) {
        super("`" + key + "`'s env `" + env + "` is `" + otherKey + "`'s indirection variable");
    }
}
