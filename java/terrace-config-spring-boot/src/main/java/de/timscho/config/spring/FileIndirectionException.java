package de.timscho.config.spring;

import org.jspecify.annotations.Nullable;

/**
 * A {@code _FILE}-suffixed environment variable named a path that could not be read.
 *
 * <p>Mirrors {@code terrace-config-loader}'s {@code LoaderException}: a message meant for an
 * operator, naming the variable at fault, never the value it would have supplied.
 */
public final class FileIndirectionException extends RuntimeException {

    private static final long serialVersionUID = 1L;

    /** Creates an exception with neither message nor cause. */
    public FileIndirectionException() {
        this(null, null);
    }

    /**
     * Creates an exception with a message and no cause.
     *
     * @param message what went wrong, naming the variable at fault and never the value
     */
    public FileIndirectionException(@Nullable final String message) {
        this(message, null);
    }

    /**
     * Creates an exception from a cause, taking the cause's message as its own.
     *
     * @param cause the underlying failure
     */
    public FileIndirectionException(@Nullable final Throwable cause) {
        this(cause == null ? null : cause.getMessage(), cause);
    }

    /**
     * Creates an exception with a message and a cause.
     *
     * @param message what went wrong, naming the variable at fault and never the value
     * @param cause the underlying failure
     */
    public FileIndirectionException(@Nullable final String message, @Nullable final Throwable cause) {
        super(message);
        if (cause != null) {
            super.initCause(cause);
        }
    }
}
