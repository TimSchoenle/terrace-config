package de.timscho.config.core.schema;

import java.util.List;

import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.LoaderVar;
import de.timscho.config.core.model.Schema;

import lombok.experimental.UtilityClass;

/**
 * The Markdown rendering: GitHub-flavoured tables, ready to paste into a README — a port of the
 * Rust crate's {@code schema::markdown} module.
 *
 * <p>The rendering whose consumer is a person reading a page, which is what every choice here is
 * about. A cell has a page width to stay inside, so {@link Column#DEFAULT_COLUMNS} is narrower
 * than the set of columns that exist; a cell is prose, so {@code |} and {@code \} are escaped
 * rather than trusted; and a cell shows the <i>summary</i> of a comment rather than the whole of
 * it.
 *
 * <p>Nothing here interprets a value: {@code 12} prints as {@code 12} and {@code public} prints
 * as {@code public}, quotes and all left off, because that is what reads well in a table.
 */
@UtilityClass
public class MarkdownRenderer {

    /**
     * Both tables: the variables the loader reads, then the configuration keys under
     * {@link Column#DEFAULT_COLUMNS}. Ends with a newline.
     */
    public static String toMarkdown(Schema schema) {
        return toMarkdownWith(schema, Column.DEFAULT_COLUMNS);
    }

    /**
     * Both tables, with a chosen set of key columns. The loader-variable table leads when there
     * is one, since an operator who cannot find {@code <PREFIX>CONFIG} cannot use any of the
     * rest. Ends with a newline.
     */
    public static String toMarkdownWith(Schema schema, List<Column> columns) {
        String loader = toMarkdownLoader(schema);
        String keys = toMarkdownKeys(schema, columns);
        if (loader.isEmpty()) {
            return keys;
        }
        // The blank line between them: two tables run together are one malformed table.
        return loader + "\n" + keys;
    }

    /**
     * The loader-variable table alone. Empty when the schema has no loader variables -- a header
     * with no rows under it would be a table promising variables that do not exist.
     */
    public static String toMarkdownLoader(Schema schema) {
        List<LoaderVar> loader = schema.getLoader();
        if (loader.isEmpty()) {
            return "";
        }

        StringBuilder out = new StringBuilder();
        out.append("| Variable | Role | Default | Purpose |\n");
        out.append("|---|---|---|---|\n");
        for (LoaderVar var : loader) {
            out.append("| `").append(escape(var.getEnv())).append("` | ")
                    .append(var.getRole().label()).append(" | ")
                    .append(optionalCode(var.getDefaultValue())).append(" | ")
                    .append(cell(var.getDocs())).append(" |\n");
        }
        return out.toString();
    }

    /**
     * The configuration-key table alone, with a chosen set of columns. A schema with no keys
     * still renders its header: an empty configuration section is a real shape, and the header
     * says the section was generated rather than forgotten.
     */
    public static String toMarkdownKeys(Schema schema, List<Column> columns) {
        StringBuilder out = new StringBuilder();
        List<String> headers = new java.util.ArrayList<>();
        for (Column column : columns) {
            headers.add(column.heading());
        }
        out.append("| ").append(String.join(" | ", headers)).append(" |\n");
        out.append("|").repeat("---|", columns.size()).append("\n");
        for (Key key : schema.getKeys()) {
            List<String> cells = new java.util.ArrayList<>();
            for (Column column : columns) {
                cells.add(column.render(key));
            }
            out.append("| ").append(String.join(" | ", cells)).append(" |\n");
        }
        return out.toString();
    }

    private static String optionalCode(String value) {
        return value == null ? "—" : "`" + escape(value) + "`";
    }

    private static String cell(String text) {
        if (text.isEmpty()) {
            return "—";
        }
        return escape(text).replace("\n", "<br>");
    }

    private static String escape(String text) {
        return text.replace("\\", "\\\\").replace("|", "\\|");
    }
}
