package de.timscho.config.core.descriptor;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.TreeMap;

import lombok.AllArgsConstructor;
import lombok.experimental.UtilityClass;
import org.jspecify.annotations.Nullable;

import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.UnreachableReason;

/**
 * Combines a dialect-agnostic {@link TypeDescriptor} with a {@link Dialect} into a {@link
 * Schema} — the Java equivalent of the Rust crate's {@code Schema::describe_at}. {@code
 * terrace-config-loader}'s {@code schema()}/{@code schema_at()} and a future Spring producer's
 * {@code Contract} both build on this, since neither has to re-derive how a key path becomes an
 * environment or secrets-file spelling.
 *
 * <p>The version of this document's shape is fixed at {@value #SCHEMA_VERSION}, matching the
 * Rust crate's own {@code SCHEMA_VERSION}.
 *
 * <p><b>Not ported yet</b>: filling in each key's observed default from a live instance (the
 * Rust crate's {@code Schema::with_defaults_from}, which needs a {@code T} to serialise and so
 * cannot be a static property of the descriptor alone) — every key here has {@code required =
 * true} unless its field is {@code Optional}, an approximation until that step exists. See
 * {@code docs/migration-progress.md} for the open item.
 */
@UtilityClass
public class SchemaAssembler {

    /** The version of this document's shape. Matches the Rust crate's {@code SCHEMA_VERSION}. */
    public static final int SCHEMA_VERSION = 2;

    /** The keys of {@code descriptor}, spelled according to {@code dialect}. */
    public static Schema assemble(TypeDescriptor descriptor, Dialect dialect, Set<String> reservedEnvNames) {
        return assemble(descriptor, dialect, reservedEnvNames, "");
    }

    /**
     * The keys of {@code descriptor}, as they are spelled when it sits at {@code root} in a
     * larger configuration — {@code schemaAt("csp")} on a type describing {@code
     * cloudflare.turnstile} produces {@code csp.cloudflare.turnstile}. An empty {@code root} is
     * {@link #assemble(TypeDescriptor, Dialect, Set)}.
     *
     * @param reservedEnvNames every environment name (full spelling, e.g. {@code MYAPP_CONFIG})
     *                         the loader itself reads before the layers exist; matched
     *                         case-insensitively, mirroring {@code Dialect::is_reserved}
     */
    public static Schema assemble(
            TypeDescriptor descriptor, Dialect dialect, Set<String> reservedEnvNames, String root) {
        Set<String> reservedUpper = new HashSet<>();
        for (String reserved : reservedEnvNames) {
            reservedUpper.add(reserved.toUpperCase(Locale.ROOT));
        }

        List<Key> keys = new ArrayList<>();
        walk(descriptor.keys(), root, keys, dialect, reservedUpper);

        return Schema.builder()
                .schemaVersion(SCHEMA_VERSION)
                .dialect(dialect)
                .keys(keys)
                .build();
    }

    /** Walks {@code fields}, appending one {@link Key} per leaf/container field to {@code out}. */
    private static void walk(
            List<KeyDescriptor> fields, String prefix, List<Key> out, Dialect dialect, Set<String> reservedUpper) {
        for (KeyDescriptor field : fields) {
            String path = prefix.isEmpty() ? field.name() : prefix + "." + field.name();
            boolean isNestedStruct = !field.nestedKeys().isEmpty()
                    && (field.container() == KeyDescriptor.ContainerKind.NONE
                            || field.container() == KeyDescriptor.ContainerKind.OPTIONAL);
            if (isNestedStruct) {
                // `#[config(nested)]`: the field opens a level rather than becoming a key of its
                // own, bare or behind an `Optional` — neither carries a key of its own in Rust
                // either, since a struct field has no scalar value to bind directly.
                walk(field.nestedKeys(), path, out, dialect, reservedUpper);
            } else {
                out.add(leaf(field, path, dialect, reservedUpper));
            }
        }
    }

    /** One field as a leaf {@link Key} — the whole of a container field, structured or not. */
    private static Key leaf(KeyDescriptor field, String path, Dialect dialect, Set<String> reservedUpper) {
        boolean container = field.container() != KeyDescriptor.ContainerKind.NONE
                && field.container() != KeyDescriptor.ContainerKind.OPTIONAL;

        List<String> values = !field.values().isEmpty()
                ? field.values()
                : field.element() != null ? field.element().values() : List.of();
        TextForm textForm = textForm(field, container, values);

        Key.KeyBuilder builder = Key.builder()
                .path(path)
                .docs(field.docs() != null ? field.docs() : "")
                .ty(field.typeName())
                .values(values)
                .constraint(constraint(field, container, textForm, values))
                .textForm(textForm.model)
                .secret(field.secret())
                .note(field.note())
                // Not derived from a live instance yet — see the class-level note.
                .required(field.container() != KeyDescriptor.ContainerKind.OPTIONAL);

        Spelling spelling = envSpelling(dialect, path);
        builder.env(spelling.env);
        boolean reserved = spelling.env != null && reservedUpper.contains(spelling.env.toUpperCase(Locale.ROOT));
        builder.reserved(reserved);
        if (reserved) {
            // Read straight from the environment, so neither file mechanism can supply it.
            builder.envFile(null);
            builder.secretsFile(null);
            builder.unreachable(spelling.unreachable);
        } else {
            builder.envFile(spelling.env != null ? indirectionName(dialect, spelling.env) : null);
            builder.secretsFile(secretsFileName(dialect, path));
            builder.unreachable(spelling.unreachable);
        }
        return builder.build();
    }

    /** How to read the field's own value, before {@link Key#getConstraint()} checks it. */
    private static TextForm textForm(KeyDescriptor field, boolean container, List<String> values) {
        if (container) {
            return TextForm.STRUCTURED;
        }
        if (!values.isEmpty()) {
            return TextForm.CHOICE;
        }
        return TextForm.of(field.typeName());
    }

    /** The JSON Schema keywords this field's value must satisfy, or {@code null} if none apply. */
    private static @Nullable Map<String, Object> constraint(
            KeyDescriptor field, boolean container, TextForm textForm, List<String> values) {
        if (container) {
            Map<String, Object> schema = new TreeMap<>();
            if (Objects.requireNonNull(field.container()) == KeyDescriptor.ContainerKind.MAP) {
                schema.put("type", "object");
            } else {
                schema.put("type", "array");
                Map<String, Object> items = elementConstraint(field.element());
                if (items != null) {
                    schema.put("items", items);
                }
            }
            return schema;
        }
        return leafConstraint(textForm, values, field.range());
    }

    private static @Nullable Map<String, Object> elementConstraint(@Nullable ElementDescriptor element) {
        if (element == null) {
            return null;
        }
        TextForm elementForm = !element.values().isEmpty() ? TextForm.CHOICE : TextForm.of(element.typeName());
        return leafConstraint(elementForm, element.values(), element.range());
    }

    private static @Nullable Map<String, Object> leafConstraint(
            TextForm textForm, List<String> values, @Nullable RangeConstraint range) {
        Map<String, Object> schema = new TreeMap<>();
        switch (textForm) {
            case CHOICE:
                schema.put("type", "string");
                schema.put("enum", new ArrayList<>(values));
                return schema;
            case TEXT:
                schema.put("type", "string");
                return schema;
            case BOOLEAN:
                schema.put("type", "boolean");
                return schema;
            case INTEGER:
                schema.put("type", "integer");
                addRange(schema, range);
                return schema;
            case NUMBER:
                schema.put("type", "number");
                addRange(schema, range);
                return schema;
            case UNKNOWN:
            default:
                // No check is possible; a consumer must not invent one.
                return range == null ? null : rangeOnly(range);
        }
    }

    private static @Nullable Map<String, Object> rangeOnly(RangeConstraint range) {
        Map<String, Object> schema = new TreeMap<>();
        addRange(schema, range);
        return schema.isEmpty() ? null : schema;
    }

    private static void addRange(Map<String, Object> schema, @Nullable RangeConstraint range) {
        if (range == null) {
            return;
        }
        if (range.min() != null) {
            schema.put("minimum", range.min());
        }
        if (range.max() != null) {
            schema.put("maximum", range.max());
        }
        if (range.exclusiveMin() != null) {
            schema.put("exclusiveMinimum", range.exclusiveMin());
        }
        if (range.exclusiveMax() != null) {
            schema.put("exclusiveMaximum", range.exclusiveMax());
        }
    }

    /** The environment spelling of {@code path}, when the environment can actually name it. */
    private static Spelling envSpelling(Dialect dialect, String path) {
        String name = dialect.getPrefix() + path.toUpperCase(Locale.ROOT).replace(".", dialect.getNestingSeparator());
        if (!isSettableEnvName(name)) {
            return new Spelling(null, UnreachableReason.UNNAMEABLE);
        }
        if (indirectionTarget(dialect, name) != null) {
            return new Spelling(null, UnreachableReason.INDIRECTION);
        }
        String mapped = envLayerKey(dialect, name);
        if (path.equals(mapped)) {
            return new Spelling(name, null);
        }
        return new Spelling(null, UnreachableReason.UNNAMEABLE);
    }

    /** The key a case-folding, separator-splitting environment reader makes of {@code name}. */
    private static @Nullable String envLayerKey(Dialect dialect, String name) {
        String trimmed = name.trim();
        if (!trimmed.startsWith(dialect.getPrefix())) {
            return null;
        }
        String suffix = trimmed.substring(dialect.getPrefix().length());
        String mapped = suffix.replace(dialect.getNestingSeparator(), ".").trim();
        for (String segment : mapped.split("\\.", -1)) {
            if (segment.isEmpty()) {
                return null;
            }
        }
        return mapped.toLowerCase(Locale.ROOT);
    }

    /** The key an indirection variable names, if {@code name} is one, or {@code null}. */
    private static @Nullable String indirectionTarget(Dialect dialect, String name) {
        if (!name.startsWith(dialect.getPrefix())) {
            return null;
        }
        String rest = name.substring(dialect.getPrefix().length());
        if (!rest.endsWith(dialect.getIndirectionSuffix())) {
            return null;
        }
        String key =
                rest.substring(0, rest.length() - dialect.getIndirectionSuffix().length());
        return key.isEmpty() ? null : key;
    }

    private static @Nullable String indirectionName(Dialect dialect, String env) {
        String candidate = env + dialect.getIndirectionSuffix();
        return isSettableEnvName(candidate) ? candidate : null;
    }

    /** The secrets-directory file name for {@code path}, when one can name it. */
    private static @Nullable String secretsFileName(Dialect dialect, String path) {
        String name = path.replace(".", dialect.getNestingSeparator());
        if (name.contains(".") || !isNameableFile(name)) {
            return null;
        }
        String[] parts = name.toLowerCase(Locale.ROOT)
                .split(
                        java.util.regex.Pattern.quote(
                                dialect.getNestingSeparator().toLowerCase(Locale.ROOT)),
                        -1);
        return String.join(".", parts).equals(path) ? name : null;
    }

    private static boolean isSettableEnvName(String name) {
        return !name.isEmpty() && name.indexOf('\0') < 0 && name.indexOf('=') < 0;
    }

    private static boolean isNameableFile(String name) {
        return !name.isEmpty() && name.indexOf('\0') < 0 && name.indexOf('/') < 0 && name.indexOf('\\') < 0;
    }

    @AllArgsConstructor
    private static final class Spelling {
        final @Nullable String env;
        final @Nullable UnreachableReason unreachable;
    }

    /** The leaf shapes {@link Key#getTextForm()} distinguishes, plus a {@code NUMBER} form this
     * assembler uses internally for a floating-point range before folding it into the model's
     * {@link de.timscho.config.core.model.TextForm#UNKNOWN} — a float's own value is still
     * {@code Unknown} in the published document, matching the Rust crate. */
    @AllArgsConstructor
    private enum TextForm {
        TEXT(de.timscho.config.core.model.TextForm.TEXT),
        INTEGER(de.timscho.config.core.model.TextForm.INTEGER),
        NUMBER(de.timscho.config.core.model.TextForm.UNKNOWN),
        BOOLEAN(de.timscho.config.core.model.TextForm.BOOLEAN),
        CHOICE(de.timscho.config.core.model.TextForm.CHOICE),
        STRUCTURED(de.timscho.config.core.model.TextForm.STRUCTURED),
        UNKNOWN(de.timscho.config.core.model.TextForm.UNKNOWN);

        final de.timscho.config.core.model.TextForm model;

        static TextForm of(String typeName) {
            return switch (typeName) {
                case "String", "CharSequence", "char", "Character" -> TEXT;
                case "boolean", "Boolean" -> BOOLEAN;
                case "byte", "short", "int", "long", "Byte", "Short", "Integer", "Long", "BigInteger" -> INTEGER;
                case "float", "double", "Float", "Double", "BigDecimal" ->
                    // A float is not certain enough to check — matches the Rust crate's own
                    // `TextForm::Unknown` for this case.
                    NUMBER;
                default -> UNKNOWN;
            };
        }
    }
}
