package de.timscho.config.core.schema;

import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.LoaderVar;
import de.timscho.config.core.model.Schema;
import java.math.BigInteger;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import lombok.experimental.UtilityClass;
import org.jspecify.annotations.Nullable;

/**
 * The {@code config.example.toml} rendering: the file an operator edits, generated rather than
 * kept — a port of the Rust crate's {@code schema::toml_example} module.
 *
 * <p>Two things separate this from the Markdown rendering, both from the output being a file
 * rather than a page: it has to parse, so every value comes from {@link Key#getDefaultValue()}
 * written as the TOML literal it is rather than a picture of it; and it has to be safe to commit,
 * so a {@link Key#isSecret()} key is rendered as a placeholder whatever its default was.
 *
 * <p>Everything with a default is commented out, so the generated file and an empty file mean
 * the same thing to the loader. What is left uncommented is exactly what has to be filled in.
 */
@UtilityClass
public class TomlExampleRenderer {

    /** The column the comments this class writes wrap at. */
    private static final int WIDTH = 96;

    private static final int MAX_DEPTH = 32;

    /** The schema as a commented {@code config.toml}, ready to be copied and edited.
     *
     * @param schema the schema to render
     */
    public static String toTomlExample(final Schema schema) {
        return toTomlExampleWith(schema, TomlExampleOptions.defaults());
    }

    /** The same file, with a chosen set of parts. See {@link TomlExampleOptions}.
     *
     * @param schema  the schema to render
     * @param options which parts of the file to render
     */
    public static String toTomlExampleWith(final Schema schema, final TomlExampleOptions options) {
        final List<String> blocks = new ArrayList<>();
        if (options.header()) {
            blocks.add(preamble(schema));
        }
        final Node root = Node.of(schema.getKeys());
        collect(blocks, root, "", options);
        return String.join("\n", blocks);
    }

    /** What the file is, and the variables that decide whether it is read at all. */
    private static String preamble(final Schema schema) {
        final String prefix = schema.getDialect().getPrefix();
        final String suffix = schema.getDialect().getIndirectionSuffix();
        final StringBuilder out = new StringBuilder();

        paragraph(out, "Configuration for a service reading " + prefix + "-prefixed keys.");
        comment(out, "");
        paragraph(
                out,
                "Generated from the configuration type, so it lists every key that type can carry "
                        + "and nothing else. Each key shows the value it already has, commented out: a "
                        + "commented key and a deleted key mean the same thing to the loader, so uncomment one "
                        + "only to change it. A key that is not commented out has no default, and nothing "
                        + "loads until something supplies it.");
        comment(out, "");
        paragraph(
                out,
                "Three layers can supply any key below, and all three win over this file: the "
                        + "environment variable named above the key, a file named by that variable plus `"
                        + suffix + "`, and a key-named file in the secrets directory. A secret belongs in "
                        + "one of those -- this file is usually committed.");

        final List<LoaderVar> loader = schema.getLoader();
        if (!loader.isEmpty()) {
            comment(out, "");
            comment(out, "Read before this file exists:");
            for (final LoaderVar var : loader) {
                comment(out, "");
                final String defaultSuffix =
                        var.getDefaultValue() != null ? ", default `" + var.getDefaultValue() + "`" : "";
                comment(out, "  " + var.getEnv() + " -- " + var.getRole().label() + defaultSuffix);
                final String summary = Docs.SUMMARY.of(var.getDocs());
                flowed(out, "    ", "    ", summary == null ? "" : summary);
            }
        }

        return out.toString();
    }

    /** Push this level's block, then every level below it. Depth first and leaves first, as TOML requires. */
    private static void collect(
            final List<String> blocks, final Node node, final String header, final TomlExampleOptions options) {
        final StringBuilder block = new StringBuilder();
        if (!header.isEmpty()) {
            block.append('[').append(header).append("]\n");
        }
        final List<String> keyBlocks = new ArrayList<>();
        for (final Key key : node.keys) {
            keyBlocks.add(keyBlock(key, node, options));
        }
        block.append(String.join("\n", keyBlocks));

        if (block.length() > 0) {
            blocks.add(block.toString());
        }

        for (final Node child : node.children) {
            final String segment = tomlKey(child.segment);
            final String childHeader = header.isEmpty() ? segment : header + "." + segment;
            collect(blocks, child, childHeader, options);
        }
    }

    /** One key: what it is for, above what it is set to. */
    private static String keyBlock(final Key key, final Node parent, final TomlExampleOptions options) {
        final StringBuilder out = new StringBuilder();

        appendDocsComment(out, key, options);
        appendTypeComment(out, key);
        appendAliasesComment(out, key);
        appendSpellingComment(out, key, options);
        appendRequirementComment(out, key);

        final String name = Node.name(key.getPath());
        final boolean shadowed = parent.opens(name);
        if (shadowed) {
            wrapped(out, "Shadowed by the table of the same name below: TOML cannot carry both.");
        }

        if (!key.isRequired() || key.isReserved() || shadowed) {
            out.append("# ");
        }
        out.append(tomlKey(name)).append(" = ").append(literal(key, options)).append('\n');

        return out.toString();
    }

    private static void appendDocsComment(final StringBuilder out, final Key key, final TomlExampleOptions options) {
        final String docs = options.docs().of(key.getDocs());
        if (docs != null) {
            for (final String line : docs.split("\n", -1)) {
                comment(out, line);
            }
        }
    }

    private static void appendTypeComment(final StringBuilder out, final Key key) {
        final List<String> values = key.getValues();
        if (key.getTy() != null && values.isEmpty()) {
            comment(out, "Type: " + key.getTy());
        } else if (key.getTy() != null) {
            comment(out, "Type: " + key.getTy() + " -- one of: " + String.join(", ", values));
        } else if (!values.isEmpty()) {
            comment(out, "One of: " + String.join(", ", values));
        }
        final List<String> entries = Refiner.requiredEntries(key);
        if (!entries.isEmpty()) {
            comment(out, "Must contain: " + String.join(", ", entries));
        }
    }

    private static void appendAliasesComment(final StringBuilder out, final Key key) {
        if (!key.getAliases().isEmpty()) {
            comment(out, "Also accepted as: " + String.join(", ", key.getAliases()));
        }
    }

    private static void appendSpellingComment(
            final StringBuilder out, final Key key, final TomlExampleOptions options) {
        if (key.isReserved()) {
            final String keyEnv = key.getEnv();
            final String env = keyEnv != null ? keyEnv : "the environment";
            wrapped(out, "Reserved: only " + env + " supplies this key; a file may not.");
        } else if (options.spellings()) {
            wrapped(out, spellings(key));
        }
    }

    private static void appendRequirementComment(final StringBuilder out, final Key key) {
        if (key.isRequired() && !key.isReserved()) {
            comment(out, "Required: nothing loads until this key is supplied.");
        }
        if (key.isSecret()) {
            comment(out, "Secret: the value below is a placeholder.");
        } else if (!key.isRequired() && key.getDefaultValue() == null) {
            comment(out, "Unset by default: the value below is only the shape.");
        }
    }

    /** The other ways this key can be supplied, as one comment line. */
    private static String spellings(final Key key) {
        final List<String> ways = new ArrayList<>();
        if (key.getEnv() != null) {
            ways.add(key.getEnv());
        }
        if (key.getEnvFile() != null) {
            ways.add(key.getEnvFile() + "=/path/to/file");
        }
        if (key.getSecretsFile() != null) {
            ways.add(key.getSecretsFile() + " in the secrets directory");
        }
        if (ways.isEmpty()) {
            return "Only this file supplies this key: no environment or secrets-directory spelling " + "reaches it.";
        }
        return "Also from: " + String.join(", ", ways);
    }

    /** The value written for a key: its default, a redaction, or a placeholder of the right shape. */
    private static String literal(final Key key, final TomlExampleOptions options) {
        if (key.isSecret()) {
            return tomlString(options.secretPlaceholder());
        }
        final Object defaultValue = key.getDefaultValue();
        if (defaultValue != null) {
            final String literal = tomlLiteral(defaultValue, 0);
            if (literal != null) {
                return literal;
            }
        }
        return placeholder(key, options.placeholder());
    }

    /** A value of the key's type that is still obviously not an answer. */
    private static String placeholder(final Key key, final String text) {
        String shape = null;
        final Map<String, Object> constraint = key.getConstraint();
        if (constraint != null && constraint.get("type") instanceof String type) {
            shape = type;
        }
        return switch (shape == null ? "" : shape) {
            case "boolean" -> "false";
            case "integer" -> "0";
            case "number" -> "0.0";
            case "array" -> "[]";
            case "object" -> "{}";
            default -> tomlString(text);
        };
    }

    /** A default value as TOML, or {@code null} for one TOML cannot carry. */
    private static @Nullable String tomlLiteral(@Nullable final Object value, final int depth) {
        if (depth > MAX_DEPTH) {
            return null;
        }
        if (value == null) {
            // TOML has no null: an absent key *is* the absent value, so there is nothing to write.
            return null;
        }
        if (value instanceof String string) {
            return tomlString(string);
        }
        if (value instanceof Boolean bool) {
            return bool.toString();
        }
        if (isIntegerType(value)) {
            return tomlInteger((Number) value);
        }
        if (isFloatingPointType(value)) {
            return tomlFloat(((Number) value).doubleValue());
        }
        if (value instanceof List<?> items) {
            return tomlLiteralList(items, depth);
        }
        if (value instanceof Map<?, ?> dict) {
            return tomlLiteralMap(dict, depth);
        }
        return null;
    }

    private static boolean isIntegerType(final Object value) {
        return value instanceof BigInteger
                || value instanceof Long
                || value instanceof Integer
                || value instanceof Short
                || value instanceof Byte;
    }

    private static boolean isFloatingPointType(final Object value) {
        return value instanceof Double || value instanceof Float;
    }

    private static @Nullable String tomlLiteralList(final List<?> items, final int depth) {
        final List<String> rendered = new ArrayList<>(items.size());
        for (final Object item : items) {
            final String literal = tomlLiteral(item, depth + 1);
            if (literal == null) {
                // An array element cannot be left out the way a table entry can.
                return null;
            }
            rendered.add(literal);
        }
        return "[" + String.join(", ", rendered) + "]";
    }

    private static String tomlLiteralMap(final Map<?, ?> dict, final int depth) {
        final List<String> rendered = new ArrayList<>();
        for (final Map.Entry<?, ?> entry : dict.entrySet()) {
            final String literal = tomlLiteral(entry.getValue(), depth + 1);
            if (literal != null) {
                rendered.add(tomlKey(String.valueOf(entry.getKey())) + " = " + literal);
            }
        }
        return rendered.isEmpty() ? "{}" : "{ " + String.join(", ", rendered) + " }";
    }

    /** A whole number as TOML, or {@code null} for one TOML's signed 64 bits cannot hold. */
    private static String tomlInteger(final Number number) {
        final BigInteger value = number instanceof BigInteger big ? big : BigInteger.valueOf(number.longValue());
        if (value.bitLength() > 63) {
            return null;
        }
        return value.toString();
    }

    /** A float as TOML: never bare digits, because bare digits are a TOML *integer*. */
    private static String tomlFloat(final double value) {
        if (Double.isNaN(value)) {
            return "nan";
        }
        if (Double.isInfinite(value)) {
            return value > 0 ? "inf" : "-inf";
        }
        final String rendered = String.valueOf(value);
        if (rendered.contains(".") || rendered.contains("e") || rendered.contains("E")) {
            return rendered;
        }
        return rendered + ".0";
    }

    /** A TOML basic string: quoted, with everything the format cannot carry raw escaped. */
    private static String tomlString(final String text) {
        final StringBuilder out = new StringBuilder(text.length() + 2);
        out.append('"');
        for (int i = 0; i < text.length(); i++) {
            final char c = text.charAt(i);
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\b' -> out.append("\\b");
                case '\t' -> out.append("\\t");
                case '\n' -> out.append("\\n");
                case '\f' -> out.append("\\f");
                case '\r' -> out.append("\\r");
                default -> {
                    if (c < ' ' || c == '\u007f') {
                        out.append(String.format("\\u%04X", (int) c));
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        out.append('"');
        return out.toString();
    }

    /** A key name as TOML spells it: bare where it can be, quoted where it cannot. */
    private static String tomlKey(final String name) {
        boolean bare = !name.isEmpty();
        if (bare) {
            for (int i = 0; i < name.length(); i++) {
                final char c = name.charAt(i);
                if (!(Character.isLetterOrDigit(c) && c < 128) && c != '_' && c != '-') {
                    bare = false;
                    break;
                }
            }
        }
        return bare ? name : tomlString(name);
    }

    /** One comment line, with no trailing space on an empty one. */
    private static void comment(final StringBuilder out, final String text) {
        if (text.isEmpty()) {
            out.append("#\n");
            return;
        }
        out.append("# ");
        for (int i = 0; i < text.length(); i++) {
            final char c = text.charAt(i);
            if (c == '\t' || (c >= ' ' && c != '\u007f')) {
                out.append(c);
            } else {
                out.append(String.format("\\u%04X", (int) c));
            }
        }
        out.append('\n');
    }

    /** Prose as comment lines, wrapped flush left. */
    private static void paragraph(final StringBuilder out, final String text) {
        flowed(out, "", "", text);
    }

    /** One assembled line, wrapped, with a hanging indent marking what is a continuation. */
    private static void wrapped(final StringBuilder out, final String text) {
        flowed(out, "", "  ", text);
    }

    /** As {@link #wrapped}, with a chosen indent on the first line and on the rest. */
    private static void flowed(final StringBuilder out, final String first, final String rest, final String text) {
        final StringBuilder line = new StringBuilder();
        String indent = first;
        for (final String word : text.split("\\s+")) {
            if (word.isEmpty()) {
                continue;
            }
            if (line.length() > 0 && line.length() + 1 + word.length() > WIDTH) {
                comment(out, line.toString());
                line.setLength(0);
                indent = rest;
            }
            if (line.length() == 0) {
                line.append(indent);
            } else {
                line.append(' ');
            }
            line.append(word);
        }
        if (line.length() > 0) {
            comment(out, line.toString());
        }
    }
}
