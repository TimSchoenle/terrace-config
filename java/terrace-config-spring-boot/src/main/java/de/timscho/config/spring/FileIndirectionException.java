package de.timscho.config.spring;

import lombok.experimental.StandardException;

/**
 * A {@code _FILE}-suffixed environment variable named a path that could not be read.
 *
 * <p>Mirrors {@code terrace-config-loader}'s {@code LoaderException}: a message meant for an
 * operator, naming the variable at fault, never the value it would have supplied.
 */
@StandardException
public final class FileIndirectionException extends RuntimeException {}
