package de.timscho.config.processor;

import org.jspecify.annotations.Nullable;

/**
 * The first-paragraph "summary" rule {@code rust/docs/SCHEMA.md}'s {@code Purpose} column applies
 * to a doc comment, ported from rustdoc's own convention: everything up to the first blank line.
 */
final class Javadocs {

    private Javadocs() {}

    static String summary(final String docs) {
        if (docs.isEmpty()) {
            return "";
        }
        final String[] lines = docs.split("\\R", -1);
        final StringBuilder summary = new StringBuilder();
        for (String line : lines) {
            final String trimmed = line.trim();
            if (trimmed.isEmpty()) {
                break;
            }
            if (!summary.isEmpty()) {
                summary.append(' ');
            }
            summary.append(trimmed);
        }
        return summary.toString();
    }

    static String normalize(@Nullable final String rawDocComment) {
        if (rawDocComment == null) {
            return "";
        }
        return rawDocComment.trim();
    }
}
