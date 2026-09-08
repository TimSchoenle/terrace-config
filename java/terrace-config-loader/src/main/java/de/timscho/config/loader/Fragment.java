package de.timscho.config.loader;

/**
 * What became of one file in the TOML layer.
 */
public sealed interface Fragment {

    /**
     * Read, and supplied this many keys. Zero is a real answer: an empty file, or one holding
     * only section headers.
     */
    record Read(int keys) implements Fragment {
        @Override
        public String toString() {
            return switch (keys) {
                case 0 -> "no keys";
                case 1 -> "1 key";
                default -> keys + " keys";
            };
        }
    }

    /**
     * Named but not there.
     *
     * <p>Not an error — an absent path is simply skipped, and a service with no configuration
     * file at all is the normal development case. It is, however, the most common reason a
     * configured path supplies nothing, which is why it is reported rather than passed over.
     */
    record Missing() implements Fragment {
        @Override
        public String toString() {
            return "missing";
        }
    }

    /**
     * There, but not parseable as TOML.
     *
     * <p><b>The reason is not reported here.</b> A TOML parse error quotes the line it failed on,
     * and this type never carries a value; {@link TerraceLoader#load(Class, java.util.Map)} fails
     * with the parser's own message, which names the file, the line and the column.
     */
    record Unreadable() implements Fragment {
        @Override
        public String toString() {
            return "not valid TOML";
        }
    }
}
