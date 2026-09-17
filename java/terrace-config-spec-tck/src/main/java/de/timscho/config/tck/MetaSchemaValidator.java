package de.timscho.config.tck;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.networknt.schema.JsonSchema;
import com.networknt.schema.JsonSchemaFactory;
import com.networknt.schema.SchemaValidatorsConfig;
import com.networknt.schema.SpecVersion;
import com.networknt.schema.ValidationMessage;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import org.jetbrains.annotations.Blocking;

/**
 * Checks a document against {@code spec/v1/contract.schema.json}.
 *
 * <p>Mirrors the two halves {@code rust/tests/spec.rs} checks: the whole envelope against the
 * schema root, and the {@code schema} object alone against {@code #/$defs/schema}, because that
 * half is published as an artefact in its own right (the {@code json} rendering). Configured to
 * resolve nothing over the network — {@code contract.schema.json} is self-contained by design,
 * and a validator that happened not to need the network is not the same guarantee as one that is
 * configured to refuse it.
 */
public final class MetaSchemaValidator {

    private static final ObjectMapper MAPPER = new ObjectMapper();

    private final JsonSchema fullSchema;
    private final JsonSchema schemaHalf;

    private MetaSchemaValidator(final JsonSchema fullSchema, final JsonSchema schemaHalf) {
        this.fullSchema = fullSchema;
        this.schemaHalf = schemaHalf;
    }

    /** Loads {@code spec/v1/contract.schema.json} and compiles both entry points. */
    @Blocking
    public static MetaSchemaValidator load() {
        return forSchema(readMetaSchema());
    }

    static MetaSchemaValidator forSchema(final JsonNode metaSchema) {
        final SchemaValidatorsConfig config = SchemaValidatorsConfig.builder().build();
        final JsonSchemaFactory factory = JsonSchemaFactory.getInstance(SpecVersion.VersionFlag.V202012);

        final JsonSchema full = factory.getSchema(metaSchema, config);
        final JsonSchema half = factory.getSchema(schemaHalfNode(metaSchema), config);
        return new MetaSchemaValidator(full, half);
    }

    /**
     * A local pointer into {@code #/$defs/schema}, not a resolver-mediated one: the definitions
     * live in the same document, so a copy with its root swapped reaches them without a registry
     * — a test that needed one would be testing the resolver, not the schema.
     */
    private static JsonNode schemaHalfNode(final JsonNode metaSchema) {
        final ObjectNode sub = metaSchema.deepCopy();
        sub.remove(List.of("type", "properties", "required", "title", "description"));
        sub.put("$id", "https://terrace.dev/spec/v1/schema-only.json");
        sub.put("$ref", "#/$defs/schema");
        return sub;
    }

    private static JsonNode readMetaSchema() {
        final Path path = SpecPaths.metaSchema();
        try {
            return MAPPER.readTree(path.toFile());
        } catch (IOException e) {
            throw new UncheckedIOException(path + " could not be read as JSON", e);
        }
    }

    /** Every error validating {@code instance} against the full envelope, empty if it is valid.
     *
     * @param instance the document to validate
     */
    public List<String> validateEnvelope(final JsonNode instance) {
        return describe(this.fullSchema.validate(instance));
    }

    /** Every error validating {@code instance} against {@code #/$defs/schema} alone.
     *
     * @param instance the {@code json_schema} half to validate
     */
    public List<String> validateSchemaHalf(final JsonNode instance) {
        return describe(this.schemaHalf.validate(instance));
    }

    private static List<String> describe(final Set<ValidationMessage> messages) {
        final List<String> described = new ArrayList<>();
        for (final ValidationMessage message : messages) {
            described.add("  at `" + message.getInstanceLocation() + "`: " + message.getMessage());
        }
        return described;
    }
}
