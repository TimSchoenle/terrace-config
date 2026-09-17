package de.timscho.config.core.contract;

import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.External;
import de.timscho.config.core.model.ExternalUnknownPolicy;
import de.timscho.config.core.model.ExternalVar;
import de.timscho.config.core.model.JsonSchemaDocument;
import de.timscho.config.core.model.Producer;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.refusal.ContractValidator;
import de.timscho.config.core.schema.JsonSchemaOptions;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import lombok.experimental.UtilityClass;
import org.jspecify.annotations.Nullable;

/**
 * Combines a built {@link Schema} with an {@link App} and a {@link Producer} into a full
 * {@link Contract} — the Java equivalent of the Rust crate's {@code Schema::into_contract}
 * followed by {@code ContractBuilder::build}.
 *
 * <p>Belongs in {@code -core} rather than in either loader, on {@link ContractValidator}'s own
 * reasoning: both {@code terrace-config-loader} and {@code terrace-config-spring-boot} publish
 * the same {@link Contract} shape, so assembling and refusing it is written once here rather
 * than twice downstream.
 */
@UtilityClass
public class ContractAssembler {

    /** {@link #assemble(Schema, App, Producer, External)} with nothing declared external.
     *
     * @param schema   the assembled schema this contract publishes
     * @param app      this contract's {@code app} metadata
     * @param producer identifies the loader that assembled this contract
     */
    public static Contract assemble(final Schema schema, final App app, final Producer producer) {
        return assemble(schema, app, producer, noExternalSurface());
    }

    /**
     * Assembles and validates a {@link Contract}.
     *
     * <p>{@link ExternalVar#getConstraint()} is derived from {@link ExternalVar#getTy()}/{@link
     * ExternalVar#getValues()} wherever a caller left it {@code null} — the escape hatch for a
     * domain type this module cannot interpret is stating the constraint outright, which this
     * method then leaves alone, exactly as the Rust crate's {@code ContractBuilder::build} does.
     * {@link ExternalVar#getTextForm()}/{@link ExternalVar#getTextConstraint()} are not derived:
     * unlike the Rust crate, {@link ExternalVar} has no default for {@code text_form}, so a
     * caller always states one explicitly.
     *
     * @param schema   the assembled schema this contract publishes
     * @param app      this contract's {@code app} metadata
     * @param producer identifies the loader that assembled this contract
     * @param external the service's declared external variable surface
     * @throws de.timscho.config.core.refusal.ContractRefusalException the first of
     *                                                                  {@code spec/v1/FORMAT.md}'s
     *                                                                  eight refusals this
     *                                                                  contract would violate
     */
    public static Contract assemble(
            final Schema schema, final App app, final Producer producer, final External external) {
        final External derivedExternal = deriveExternalConstraints(external);

        final JsonSchemaOptions options = JsonSchemaOptions.forContract().orTitle(app.getName() + " configuration");
        final Map<String, Object> rendered = schema.toJsonSchemaWith(options);

        final Contract contract = Contract.builder()
                .producer(producer)
                .app(app)
                .schema(schema)
                .jsonSchema(JsonSchemaDocument.of(rendered))
                .external(derivedExternal)
                .build();

        ContractValidator.validate(contract);
        return contract;
    }

    /** Nothing declared, nothing ignored, unknown variables refused — the strictest default. */
    public static External noExternalSurface() {
        return External.builder().unknown(ExternalUnknownPolicy.REJECT).build();
    }

    private static External deriveExternalConstraints(final External external) {
        final List<ExternalVar> derived = new ArrayList<>();
        for (final ExternalVar var : external.getEnv()) {
            if (var.getConstraint() == null) {
                final Map<String, Object> constraint = deriveConstraint(var.getTy(), var.getValues());
                derived.add(
                        constraint == null
                                ? var
                                : var.toBuilder().constraint(constraint).build());
            } else {
                derived.add(var);
            }
        }
        return external.toBuilder().env(derived).build();
    }

    /**
     * What a value of {@code ty} must be, as JSON Schema keywords — the same mapping {@link
     * de.timscho.config.core.descriptor.SchemaAssembler} uses for a described key's own type,
     * applied here to a hand-declared {@link ExternalVar}. {@code null} means unconstrained: a
     * type this module does not recognise, published as an unchecked existence-only declaration.
     */
    // Fully qualified to avoid colliding with this file's own `Contract` (the model type) import.
    @org.jetbrains.annotations.Contract(pure = true)
    private static @Nullable Map<String, Object> deriveConstraint(
            @Nullable final String ty, final List<String> values) {
        if (!values.isEmpty()) {
            final Map<String, Object> schema = new TreeMap<>();
            schema.put("type", "string");
            schema.put("enum", new ArrayList<>(values));
            return schema;
        }
        if (ty == null) {
            return null;
        }
        final String type =
                switch (ty) {
                    case "String", "CharSequence", "char", "Character" -> "string";
                    case "boolean", "Boolean" -> "boolean";
                    case "byte", "short", "int", "long", "Byte", "Short", "Integer", "Long", "BigInteger" -> "integer";
                    case "float", "double", "Float", "Double", "BigDecimal" -> "number";
                    default -> null;
                };
        if (type == null) {
            return null;
        }
        final Map<String, Object> schema = new TreeMap<>();
        schema.put("type", type);
        return schema;
    }
}
