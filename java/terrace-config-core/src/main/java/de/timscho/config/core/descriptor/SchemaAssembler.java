package de.timscho.config.core.descriptor;

import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.schema.JsonSchemaRenderer;
import de.timscho.config.core.schema.Spellings;
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
 * <p>{@link de.timscho.config.core.model.Key#isRequired()} is {@code false} whenever the field is
 * {@code Optional}-wrapped or {@link KeyDescriptor#hasDefault()} — the Java equivalent of the
 * Rust derive macro's own {@code !(opts.has_serde_default || container.field_default ||
 * is_option(ty))}, computed the same way: syntactically, from the field's own declaration, not
 * from a live instance. Filling in what that default actually *is* still needs one — see {@link
 * de.timscho.config.core.model.Schema#withDefaultsFromValue}, the Java port of {@code
 * Schema::with_defaults_from_value}, which a caller runs against a default-constructed instance
 * converted to a nested map.
 *
 * <p>A container key's {@link Key#getConstraint()} carries the shape of one element, as the Rust
 * crate's {@code rust_type::interpret_with} does: {@code items} for a {@code List}/{@code Set},
 * {@code additionalProperties} for a {@code Map}. A scalar element is its type's own constraint;
 * an {@code @Element} struct is its keys as nested {@code properties} with {@code required}, plus
 * {@code additionalProperties: false} wherever the type is closed — see {@link
 * JsonSchemaRenderer#elementObject}.
 */
@UtilityClass
public class SchemaAssembler {

    /** The version of this document's shape. Matches the Rust crate's {@code SCHEMA_VERSION}. */
    public static final int SCHEMA_VERSION = 2;

    /** The keys of {@code descriptor}, spelled according to {@code dialect}.
     *
     * @param descriptor       the configuration type's own field-level descriptor
     * @param dialect          the nesting-separator and indirection-suffix conventions to spell keys with
     * @param reservedEnvNames every environment name (full spelling, e.g. {@code MYAPP_CONFIG})
     *                         the loader itself reads before the layers exist; matched
     *                         case-insensitively, mirroring {@code Dialect::is_reserved}
     */
    public static Schema assemble(
            final TypeDescriptor descriptor, final Dialect dialect, final Set<String> reservedEnvNames) {
        return assemble(descriptor, dialect, reservedEnvNames, "");
    }

    /**
     * The keys of {@code descriptor}, as they are spelled when it sits at {@code root} in a
     * larger configuration — {@code schemaAt("csp")} on a type describing {@code
     * cloudflare.turnstile} produces {@code csp.cloudflare.turnstile}. An empty {@code root} is
     * {@link #assemble(TypeDescriptor, Dialect, Set)}.
     *
     * @param descriptor       the configuration type's own field-level descriptor
     * @param dialect          the nesting-separator and indirection-suffix conventions to spell keys with
     * @param reservedEnvNames every environment name (full spelling, e.g. {@code MYAPP_CONFIG})
     *                         the loader itself reads before the layers exist; matched
     *                         case-insensitively, mirroring {@code Dialect::is_reserved}
     * @param root             where {@code descriptor} sits in the larger configuration, dotted, empty at the root
     */
    public static Schema assemble(
            final TypeDescriptor descriptor,
            final Dialect dialect,
            final Set<String> reservedEnvNames,
            final String root) {
        final Set<String> reservedUpper = new HashSet<>();
        for (final String reserved : reservedEnvNames) {
            reservedUpper.add(reserved.toUpperCase(Locale.ROOT));
        }

        final List<Key> keys = new ArrayList<>();
        walk(descriptor.keys(), root, keys, dialect, reservedUpper);

        return Schema.builder()
                .schemaVersion(SCHEMA_VERSION)
                .dialect(dialect)
                .keys(keys)
                .build();
    }

    /** Walks {@code fields}, appending one {@link Key} per leaf/container field to {@code out}. */
    private static void walk(
            final List<KeyDescriptor> fields,
            final String prefix,
            final List<Key> out,
            final Dialect dialect,
            final Set<String> reservedUpper) {
        for (final KeyDescriptor field : fields) {
            final String path = prefix.isEmpty() ? field.name() : prefix + "." + field.name();
            if (isNestedStruct(field)) {
                // `#[config(nested)]`: the field opens a level rather than becoming a key of its
                // own, bare or behind an `Optional` — neither carries a key of its own in Rust
                // either, since a struct field has no scalar value to bind directly.
                walk(field.nestedKeys(), path, out, dialect, reservedUpper);
            } else {
                out.add(leaf(field, prefix, path, dialect, reservedUpper));
            }
        }
    }

    /** Whether {@code field} opens a level of its own rather than becoming a key: {@code @Nested},
     * bare or behind an {@code Optional}. */
    private static boolean isNestedStruct(final KeyDescriptor field) {
        return !field.nestedKeys().isEmpty()
                && (field.container() == KeyDescriptor.ContainerKind.NONE
                        || field.container() == KeyDescriptor.ContainerKind.OPTIONAL);
    }

    /** One field as a leaf {@link Key} — the whole of a container field, structured or not. */
    private static Key leaf(
            final KeyDescriptor field,
            final String prefix,
            final String path,
            final Dialect dialect,
            final Set<String> reservedUpper) {
        final boolean container = field.container() != KeyDescriptor.ContainerKind.NONE
                && field.container() != KeyDescriptor.ContainerKind.OPTIONAL;

        final List<String> values = !field.values().isEmpty()
                ? field.values()
                : field.element() != null ? field.element().values() : List.of();
        final TextForm textForm = textForm(field, container, values);

        final AliasSet aliasSet = resolveAliases(field, prefix, dialect);

        final Map<String, Object> textConstraint = textConstraint(field, container, textForm, values);

        final Key.KeyBuilder builder = Key.builder()
                .path(path)
                .docs(field.docs() != null ? field.docs() : "")
                .ty(field.typeName())
                .values(values)
                .constraint(constraint(field, container, textForm, values, dialect))
                .textForm(textForm.model)
                .aliases(aliasSet.paths())
                .envAliases(aliasSet.env())
                .envFileAliases(aliasSet.envFile())
                .secretsFileAliases(aliasSet.secretsFile())
                .secret(field.secret())
                .note(field.note())
                // Syntactic, matching the Rust derive macro exactly — see the class-level note.
                .required(field.container() != KeyDescriptor.ContainerKind.OPTIONAL && !field.hasDefault());

        if (textConstraint != null) {
            builder.textConstraint(textConstraint);
        }

        applyEnvSpelling(builder, dialect, path, reservedUpper);
        return builder.build();
    }

    /** {@code field}'s own aliases, spelled as an env var / env-indirection-file / secrets-file
     * name wherever the dialect can express them — {@code null} exactly where {@link Spellings#envSpelling}
     * and friends already say a plain alias has no such spelling. */
    private static AliasSet resolveAliases(final KeyDescriptor field, final String prefix, final Dialect dialect) {
        final List<String> aliases = new ArrayList<>();
        final List<String> envAliases = new ArrayList<>();
        final List<String> envFileAliases = new ArrayList<>();
        final List<String> secretsFileAliases = new ArrayList<>();
        for (final String alias : field.aliases()) {
            final String aliasPath = prefix.isEmpty() ? alias : prefix + "." + alias;
            aliases.add(aliasPath);
            final Spellings.Spelling aliasSpelling = Spellings.envSpelling(dialect, aliasPath);
            if (aliasSpelling.getEnv() != null) {
                envAliases.add(aliasSpelling.getEnv());
                final String envFile = Spellings.indirectionName(dialect, aliasSpelling.getEnv(), aliasPath);
                if (envFile != null) {
                    envFileAliases.add(envFile);
                }
            }
            final String secretFile = Spellings.secretsFileName(dialect, aliasPath);
            if (secretFile != null) {
                secretsFileAliases.add(secretFile);
            }
        }
        return new AliasSet(aliases, envAliases, envFileAliases, secretsFileAliases);
    }

    /** {@link #leaf}'s four alias lists, bundled so {@link #resolveAliases} has one return value. */
    private record AliasSet(List<String> paths, List<String> env, List<String> envFile, List<String> secretsFile) {}

    /** Sets {@code builder}'s {@code env}/{@code reserved}/{@code envFile}/{@code secretsFile}/
     * {@code unreachable} from {@code path}'s own env spelling — reserved names are read straight
     * from the environment, so neither file mechanism can supply them. */
    private static void applyEnvSpelling(
            final Key.KeyBuilder builder, final Dialect dialect, final String path, final Set<String> reservedUpper) {
        final Spellings.Spelling spelling = Spellings.envSpelling(dialect, path);
        builder.env(spelling.getEnv());
        final boolean reserved = spelling.getEnv() != null
                && reservedUpper.contains(spelling.getEnv().toUpperCase(Locale.ROOT));
        builder.reserved(reserved);
        if (reserved) {
            builder.envFile(null);
            builder.secretsFile(null);
        } else {
            builder.envFile(
                    spelling.getEnv() != null ? Spellings.indirectionName(dialect, spelling.getEnv(), path) : null);
            builder.secretsFile(Spellings.secretsFileName(dialect, path));
        }
        builder.unreachable(spelling.getUnreachable());
    }

    private static @Nullable Map<String, Object> textConstraint(
            final KeyDescriptor field, final boolean container, final TextForm textForm, final List<String> values) {
        if (container) {
            final Map<String, Object> schema = new TreeMap<>();
            schema.put("pattern", "^\\s*[\\[\\{][\\s\\S]*[\\]\\}]\\s*$");
            schema.put("type", "string");
            return schema;
        }
        return switch (textForm) {
            case CHOICE -> choiceTextConstraint(values);
            case BOOLEAN -> {
                final Map<String, Object> schema = new TreeMap<>();
                schema.put("pattern", "^\\s*(true|false)\\s*$");
                schema.put("type", "string");
                yield schema;
            }
            case INTEGER -> {
                final Map<String, Object> schema = new TreeMap<>();
                final boolean signed =
                        !field.typeName().startsWith("u") && !field.typeName().startsWith("NonZeroU");
                final String sign = signed ? "[-+]?" : "\\+?";
                schema.put("pattern", "^\\s*" + sign + "[0-9]+\\s*$");
                schema.put("type", "string");
                yield schema;
            }
            case STRUCTURED -> {
                final Map<String, Object> schema = new TreeMap<>();
                schema.put("pattern", "^\\s*[\\[\\{][\\s\\S]*[\\]\\}]\\s*$");
                schema.put("type", "string");
                yield schema;
            }
            case TEXT, NUMBER, UNKNOWN -> null;
        };
    }

    private static Map<String, Object> choiceTextConstraint(final List<String> values) {
        final Map<String, Object> schema = new TreeMap<>();
        final StringBuilder alternatives = new StringBuilder();
        for (int i = 0; i < values.size(); i++) {
            if (i > 0) {
                alternatives.append("|");
            }
            alternatives.append(escapeRegex(values.get(i)));
        }
        schema.put("pattern", "^\\s*(" + alternatives + ")\\s*$");
        schema.put("type", "string");
        return schema;
    }

    private static String escapeRegex(final String value) {
        final StringBuilder escaped = new StringBuilder(value.length());
        final String special = "\\^$.|?*+()[]{}";
        for (int i = 0; i < value.length(); i++) {
            final char c = value.charAt(i);
            if (special.indexOf(c) >= 0) {
                escaped.append('\\');
            }
            escaped.append(c);
        }
        return escaped.toString();
    }

    /** How to read the field's own value, before {@link Key#getConstraint()} checks it. */
    private static TextForm textForm(final KeyDescriptor field, final boolean container, final List<String> values) {
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
            final KeyDescriptor field,
            final boolean container,
            final TextForm textForm,
            final List<String> values,
            final Dialect dialect) {
        if (!container) {
            return leafConstraint(textForm, values, field.range());
        }
        final Map<String, Object> schema = new TreeMap<>();
        final Map<String, Object> element = elementSchema(field, dialect);
        if (Objects.requireNonNull(field.container()) == KeyDescriptor.ContainerKind.MAP) {
            // The map's key type is ignored on purpose: a TOML table's keys are strings whatever
            // the map is keyed by, so the element schema describes the value and nothing else.
            schema.put("type", "object");
            if (element != null) {
                schema.put("additionalProperties", element);
            }
        } else {
            schema.put("type", "array");
            if (element != null) {
                schema.put("items", element);
            }
        }
        return schema;
    }

    /**
     * One element of the container {@code field}, as JSON Schema — the position the Rust crate's
     * {@code rust_type::interpret_with} fills. {@code null} where nothing certain is known: no
     * {@link ElementDescriptor} at all, or a scalar type {@link TextForm#of} does not recognise.
     *
     * <p>A {@code @TerraceConfig} struct element is its own keys as one object schema, built by
     * the same {@link #walk} as the document's keys and rendered by {@link
     * JsonSchemaRenderer#elementObject}. Each of those keys already carries its own composed
     * constraint, so a container field inside the element nests its element schema in turn, as
     * deep as the types stack.
     */
    private static @Nullable Map<String, Object> elementSchema(final KeyDescriptor field, final Dialect dialect) {
        final ElementDescriptor element = field.element();
        if (element == null) {
            return null;
        }
        if (element.nestedKeys().isEmpty()) {
            final TextForm elementForm =
                    !element.values().isEmpty() ? TextForm.CHOICE : TextForm.of(element.typeName());
            return leafConstraint(elementForm, element.values(), element.range());
        }

        // Walked with no reserved names: an element's fields have no environment spelling that the
        // loader could read ahead of the layers, and only their constraints survive into the result.
        final List<Key> keys = new ArrayList<>();
        walk(element.nestedKeys(), "", keys, dialect, Set.of());
        final Set<String> closed = new HashSet<>();
        if (field.closed()) {
            closed.add("");
        }
        closedLevels(element.nestedKeys(), "", closed);
        return JsonSchemaRenderer.elementObject(keys, closed);
    }

    /** Adds to {@code out} the path of every {@code @Nested} level under {@code prefix} whose type
     * refuses an undeclared key — the levels an element schema closes on the type's own word. */
    private static void closedLevels(final List<KeyDescriptor> fields, final String prefix, final Set<String> out) {
        for (final KeyDescriptor field : fields) {
            if (!isNestedStruct(field)) {
                continue;
            }
            final String path = prefix.isEmpty() ? field.name() : prefix + "." + field.name();
            if (field.closed()) {
                out.add(path);
            }
            closedLevels(field.nestedKeys(), path, out);
        }
    }

    private static @Nullable Map<String, Object> leafConstraint(
            final TextForm textForm, final List<String> values, @Nullable final RangeConstraint range) {
        final Map<String, Object> schema = new TreeMap<>();
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

    private static @Nullable Map<String, Object> rangeOnly(final RangeConstraint range) {
        final Map<String, Object> schema = new TreeMap<>();
        addRange(schema, range);
        return schema.isEmpty() ? null : schema;
    }

    private static void addRange(final Map<String, Object> schema, @Nullable final RangeConstraint range) {
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

    /** The leaf shapes {@link Key#getTextForm()} distinguishes, plus a {@code NUMBER} form this
     * assembler uses internally for a floating-point range before folding it into the model's
     * {@link de.timscho.config.core.model.TextForm#UNKNOWN} — a float's own value is still
     * {@code Unknown} in the published document, matching the Rust crate. */
    // VisibilityModifier: `model` carries no access modifier in source and relies on
    // java/lombok.config's project-wide `lombok.fieldDefaults.defaultPrivate = true`, a transform
    // Checkstyle cannot see because it reads source before Lombok runs.
    @SuppressWarnings("checkstyle:VisibilityModifier")
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

        // CHECKSTYLE.OFF: Indentation -- palantirJavaFormat wraps this arrow-case body at 4 spaces
        // past `case`; the fetched ruleset's Indentation check wants 8. Reformatting by hand would
        // just be undone by the next spotlessApply, so this scoped disable defers to the formatter
        // that actually governs this file (see terrace-config.java-conventions.gradle.kts'
        // checkstyle block).
        static TextForm of(final String typeName) {
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
        // CHECKSTYLE.ON: Indentation
    }
}
