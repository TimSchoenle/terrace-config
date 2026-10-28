package de.timscho.config.loader;

import org.jspecify.annotations.Nullable;

/**
 * Something the loader could not do: a configured source could not be read, a file was not valid
 * UTF-8, or one key was supplied by more than one mechanism under {@link ShadowPolicy#REJECT}.
 *
 * <p>Mirrors the Rust crate's {@code Error::Source}: a message meant for an operator, naming the
 * path or variable at fault, never the value.
 */
public final class LoaderException extends RuntimeException {

    private static final long serialVersionUID = 1L;

    /** Creates an exception with neither message nor cause. */
    public LoaderException() {
        this(null, null);
    }

    /**
     * Creates an exception with a message and no cause.
     *
     * @param message what went wrong, naming the path or variable at fault and never the value
     */
    public LoaderException(@Nullable final String message) {
        this(message, null);
    }

    /**
     * Creates an exception from a cause, taking the cause's message as its own.
     *
     * @param cause the underlying failure
     */
    public LoaderException(@Nullable final Throwable cause) {
        this(cause == null ? null : cause.getMessage(), cause);
    }

    /**
     * Creates an exception with a message and a cause.
     *
     * @param message what went wrong, naming the path or variable at fault and never the value
     * @param cause the underlying failure
     */
    public LoaderException(@Nullable final String message, @Nullable final Throwable cause) {
        super(message);
        if (cause != null) {
            super.initCause(cause);
        }
    }
}
