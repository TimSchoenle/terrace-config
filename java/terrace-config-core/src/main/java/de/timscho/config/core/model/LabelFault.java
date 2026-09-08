package de.timscho.config.core.model;

/**
 * One image label {@link Contract#checkLabels} finds wrong — either missing entirely, or present
 * with a different value than the contract expects.
 */
public abstract sealed class LabelFault {

    private final String name;

    private LabelFault(String name) {
        this.name = name;
    }

    /** The label the contract expected, one of {@link Contract#LABEL_VERSION}, {@link Contract#LABEL_PATH}
     * or {@link Contract#LABEL_PREFIX}. */
    public String getName() {
        return name;
    }

    public static LabelFault missing(String name) {
        return new Missing(name);
    }

    public static LabelFault mismatch(String name, String found, String expected) {
        return new Mismatch(name, found, expected);
    }

    /** The image carries no label of this name at all. */
    public static final class Missing extends LabelFault {
        private Missing(String name) {
            super(name);
        }

        @Override
        public String toString() {
            return "the image carries no `" + getName() + "`, so nothing can discover this contract from "
                    + "its config blob.";
        }
    }

    /** The image carries the label with a different value. */
    public static final class Mismatch extends LabelFault {
        private final String found;
        private final String expected;

        private Mismatch(String name, String found, String expected) {
            super(name);
            this.found = found;
            this.expected = expected;
        }

        /** What the image carries. */
        public String getFound() {
            return found;
        }

        /** What this contract says it should carry. */
        public String getExpected() {
            return expected;
        }

        @Override
        public String toString() {
            return "the image's `" + getName() + "` is `" + found + "`, and this contract's is `" + expected
                    + "`.";
        }
    }
}
