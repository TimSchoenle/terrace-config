package de.timscho.config.loader;

import java.nio.file.Path;

/**
 * One layer, and the file or variable inside it that carried a value.
 *
 * <p>Exhaustive on purpose, mirroring the Rust crate's own {@code explain::Layer}: these are the
 * layers this module documents, a fifth one would be a change nobody could miss, and code
 * switching over every case should get a compile error when that day comes rather than silently
 * falling through a {@code default}.
 */
public sealed interface Layer {

    /** A TOML fragment: the file {@code $<PREFIX>CONFIG} names, or one of a directory's {@code *.toml} files. */
    record Toml(Path path) implements Layer {
        @Override
        public String toString() {
            return "TOML " + path;
        }
    }

    /** A {@code <PREFIX>}-prefixed environment variable, in the spelling it was set in. */
    record Env(String var) implements Layer {
        @Override
        public String toString() {
            return "environment " + var;
        }
    }

    /** A key-named file directly inside {@code $<PREFIX>SECRETS_DIR}. */
    record SecretsFile(Path path) implements Layer {
        @Override
        public String toString() {
            return "secrets file " + path;
        }
    }

    /** {@code <PREFIX><KEY><SUFFIX>} indirection. */
    record Indirection(String var, Path path) implements Layer {
        @Override
        public String toString() {
            return "indirection " + var + " -> " + path;
        }
    }
}
