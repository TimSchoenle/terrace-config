package de.timscho.config.core.schema;

import org.jspecify.annotations.Nullable;

/**
 * How much of a key's documentation comment a rendering carries into a JSON Schema {@code
 * description} — ported from the Rust crate's {@code Docs}, which the Markdown and TOML
 * renderings share; this rendering is the only Java consumer so far.
 */
public enum Docs {

    /** Leave the comment out. */
    NONE,

    /** The summary alone: the first paragraph, soft wraps flattened to spaces. */
    SUMMARY,

    /** The whole comment, its paragraphs and line breaks intact. */
    FULL;

    /** The text this setting takes from {@code docs}, or {@code null} when there is nothing to take. */
    public @Nullable String of(String docs) {
        String text;
        switch (this) {
            case NONE:
                return null;
            case SUMMARY:
                text = summary(docs);
                break;
            case FULL:
            default:
                text = stripTrailingWhitespace(docs);
                break;
        }
        return text.isEmpty() ? null : text;
    }

    /** The first paragraph of {@code docs}, on one line, soft wraps turned into spaces. */
    private static String summary(String docs) {
        StringBuilder summary = new StringBuilder();
        for (String line : docs.split("\n", -1)) {
            if (line.trim().isEmpty()) {
                break;
            }
            if (!summary.isEmpty()) {
                summary.append(' ');
            }
            summary.append(line);
        }
        return summary.toString();
    }

    private static String stripTrailingWhitespace(String docs) {
        int end = docs.length();
        while (end > 0 && Character.isWhitespace(docs.charAt(end - 1))) {
            end--;
        }
        return docs.substring(0, end);
    }
}
