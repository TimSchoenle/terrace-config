package de.timscho.config.core.model;

import lombok.AccessLevel;
import lombok.Getter;
import lombok.RequiredArgsConstructor;

/**
 * One image label {@link Contract#checkLabels} finds wrong — either missing entirely, or present
 * with a different value than the contract expects.
 */
@Getter
@RequiredArgsConstructor(access = AccessLevel.PRIVATE)
public abstract sealed class LabelFault {

    /** The label the contract expected, one of {@link Contract#LABEL_VERSION}, {@link Contract#LABEL_PATH}
     * or {@link Contract#LABEL_PREFIX}. */
    private final String name;

    public static LabelFault missing(final String name) {
        return new Missing(name);
    }

    public static LabelFault mismatch(final String name, final String found, final String expected) {
        return new Mismatch(name, found, expected);
    }

    /** The image carries no label of this name at all. */
    public static final class Missing extends LabelFault {
        private Missing(final String name) {
            super(name);
        }

        @Override
        public String toString() {
            return "the image carries no `" + getName() + "`, so nothing can discover this contract from "
                    + "its config blob.";
        }
    }

    /** The image carries the label with a different value. */
    @Getter
    public static final class Mismatch extends LabelFault {
        /** What the image carries. */
        private final String found;

        /** What this contract says it should carry. */
        private final String expected;

        private Mismatch(final String name, final String found, final String expected) {
            super(name);
            this.found = found;
            this.expected = expected;
        }

        @Override
        public String toString() {
            return "the image's `" + getName() + "` is `" + found + "`, and this contract's is `" + expected + "`.";
        }
    }
}
