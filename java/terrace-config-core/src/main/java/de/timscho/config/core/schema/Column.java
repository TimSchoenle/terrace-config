package de.timscho.config.core.schema;

import java.util.List;

import de.timscho.config.core.model.Key;

/**
 * One column of the Markdown key table — ported from the Rust crate's {@code schema::markdown::Column}.
 *
 * <p>The full set is deliberately wider than {@link #DEFAULT}: everything is available to a
 * caller who wants it, and the default stays narrow enough to read.
 */
public enum Column {

    /** The TOML/document key path. */
    PATH,
    /** What kind of value the key takes -- its type, or the choices it accepts. */
    TYPE,
    /** Other key paths that supply the same key. */
    ALIASES,
    /** The environment variable supplying the value directly. */
    ENV,
    /** The variable naming a file holding the value. */
    ENV_FILE,
    /** The file name inside the secrets directory. */
    SECRETS_FILE,
    /** The value when nothing supplies the key, with its note in parentheses. */
    DEFAULT,
    /** That value alone, with no note folded in. Pair it with {@link #NOTE}. */
    DEFAULT_VALUE,
    /** The note prose on its own, for a table that keeps the two apart. */
    NOTE,
    /** {@code required}, {@code secret} and {@code reserved}, collapsed into one cell. */
    FLAGS,
    /** Whether the key must be supplied. */
    REQUIRED,
    /** Whether the value is secret. */
    SECRET,
    /** The {@code ///} comment. */
    DOCS;

    /**
     * The columns {@link de.timscho.config.core.model.Schema#toMarkdown()} emits: everything an
     * operator needs, and nothing that pushes the table past the width of a page.
     */
    public static final List<Column> DEFAULT_COLUMNS = List.of(PATH, TYPE, ENV, DEFAULT, FLAGS, DOCS);

    String heading() {
        return switch (this) {
            case PATH -> "TOML";
            case TYPE -> "Type";
            case ALIASES -> "Also accepts";
            case ENV -> "Environment";
            case ENV_FILE -> "File indirection";
            case SECRETS_FILE -> "Secrets file";
            case DEFAULT, DEFAULT_VALUE -> "Default";
            case NOTE -> "Note";
            case FLAGS -> "Flags";
            case REQUIRED -> "Required";
            case SECRET -> "Secret";
            case DOCS -> "Purpose";
        };
    }

    String render(Key key) {
        return switch (this) {
            case PATH -> "`" + escape(key.getPath()) + "`";
            case TYPE -> renderType(key);
            case ALIASES -> renderAliases(key);
            case ENV -> optionalCode(key.getEnv());
            case ENV_FILE -> optionalCode(key.getEnvFile());
            case SECRETS_FILE -> optionalCode(key.getSecretsFile());
            case DEFAULT -> renderDefault(key, true);
            case DEFAULT_VALUE -> renderDefault(key, false);
            case NOTE -> key.getNote() == null ? "\u2014" : cell(key.getNote());
            case FLAGS -> renderFlags(key);
            case REQUIRED -> yesOrDash(key.isRequired());
            case SECRET -> yesOrDash(key.isSecret());
            case DOCS -> summaryCell(key.getDocs());
        };
    }

    private static String renderType(Key key) {
        List<String> values = key.getValues();
        if (values.isEmpty()) {
            return optionalCode(key.getTy());
        }
        StringBuilder choices = new StringBuilder();
        for (String value : values) {
            if (choices.length() > 0) {
                choices.append(" \\| ");
            }
            choices.append('`').append(escape(value)).append('`');
        }
        return key.getTy() != null ? "`" + escape(key.getTy()) + "`: " + choices : choices.toString();
    }

    private static String renderAliases(Key key) {
        List<String> aliases = key.getAliases();
        if (aliases.isEmpty()) {
            return "\u2014";
        }
        StringBuilder out = new StringBuilder();
        for (String alias : aliases) {
            if (out.length() > 0) {
                out.append(", ");
            }
            out.append('`').append(escape(alias)).append('`');
        }
        return out.toString();
    }

    private static String renderDefault(Key key, boolean withNote) {
        String value;
        if (key.getDefaultText() != null) {
            value = "`" + escape(key.getDefaultText()) + "`";
        } else if (key.isRequired()) {
            value = "\u2014";
        } else {
            value = "unset";
        }
        if (withNote && key.getNote() != null) {
            return value + " (" + cell(key.getNote()) + ")";
        }
        return value;
    }

    private static String renderFlags(Key key) {
        List<String> notes = new java.util.ArrayList<>();
        if (key.isRequired()) {
            notes.add("required");
        }
        if (key.isSecret()) {
            notes.add("secret");
        }
        if (key.isReserved()) {
            notes.add("reserved");
        }
        return notes.isEmpty() ? "\u2014" : String.join(", ", notes);
    }

    private static String yesOrDash(boolean flag) {
        return flag ? "yes" : "\u2014";
    }

    /** A spelling as inline code, or an em dash when there is none. */
    private static String optionalCode(String value) {
        return value == null ? "\u2014" : "`" + escape(value) + "`";
    }

    /** Prose in a table cell: newlines become breaks, and {@code |} stops ending the cell early. */
    private static String cell(String text) {
        if (text.isEmpty()) {
            return "\u2014";
        }
        return escape(text).replace("\n", "<br>");
    }

    /** A doc comment in a table cell: its summary, on one line. */
    private static String summaryCell(String text) {
        String summary = Docs.SUMMARY.of(text);
        return summary == null ? "\u2014" : escape(summary);
    }

    /** The characters that would otherwise be read as table structure. */
    private static String escape(String text) {
        return text.replace("\\", "\\\\").replace("|", "\\|");
    }
}
