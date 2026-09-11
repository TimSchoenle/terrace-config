package de.timscho.config.loader;

import lombok.experimental.StandardException;

/**
 * Something the loader could not do: a configured source could not be read, a file was not valid
 * UTF-8, or one key was supplied by more than one mechanism under {@link ShadowPolicy#REJECT}.
 *
 * <p>Mirrors the Rust crate's {@code Error::Source} — a message meant for an operator, naming the
 * path or variable at fault, never the value.
 */
@StandardException
public final class LoaderException extends RuntimeException {
}
