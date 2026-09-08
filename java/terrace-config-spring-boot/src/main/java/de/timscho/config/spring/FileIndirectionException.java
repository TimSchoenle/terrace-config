package de.timscho.config.spring;

/**
 * A {@code _FILE}-suffixed environment variable named a path that could not be read.
 *
 * <p>Mirrors {@code terrace-config-loader}'s {@code LoaderException}: a message meant for an
 * operator, naming the variable at fault, never the value it would have supplied.
 */
public final class FileIndirectionException extends RuntimeException {

    public FileIndirectionException(String message) {
        super(message);
    }

    public FileIndirectionException(String message, Throwable cause) {
        super(message, cause);
    }
}
