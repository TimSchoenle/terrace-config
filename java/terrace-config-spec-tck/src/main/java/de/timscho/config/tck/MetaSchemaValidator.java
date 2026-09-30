package de.timscho.config.tck;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.networknt.schema.InputFormat;
import com.networknt.schema.Schema;
import com.networknt.schema.SchemaRegistry;
import com.networknt.schema.SpecificationVersion;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
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
 *
 * <p>The validator is built on Jackson 3 while this module's public surface, and every caller of
 * it, speaks Jackson 2. Documents cross that boundary as JSON text rather than through a tree
 * conversion: text is the one representation both majors agree on exactly, and it keeps Jackson 3
 * an implementation detail of the validator instead of a second tree model on this API.
 */
public final class MetaSchemaValidator {

    private static final ObjectMapper MAPPER = new ObjectMapper();

    private final Schema fullSchema;
    private final Schema schemaHalf;

    private MetaSchemaValidator(final Schema fullSchema, final Schema schemaHalf) {
        this.fullSchema = fullSchema;
        this.schemaHalf = schemaHalf;
    }

    /** Loads {@code spec/v1/contract.schema.json} and compiles both entry points. */
    @Blocking
    public static MetaSchemaValidator load() {
        return forSchema(readMetaSchema());
    }

    static MetaSchemaValidator forSchema(final JsonNode metaSchema) {
        final SchemaRegistry registry = SchemaRegistry.withDefaultDialect(
                SpecificationVersion.DRAFT_2020_12,
                builder -> builder.schemaLoader(loader -> loader.fetchRemoteResources(false)));

        final Schema full = registry.getSchema(toJson(metaSchema), InputFormat.JSON);
        final Schema half = registry.getSchema(toJson(schemaHalfNode(metaSchema)), InputFormat.JSON);
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
        return describe(this.fullSchema.validate(toJson(instance), InputFormat.JSON));
    }

    /** Every error validating {@code instance} against {@code #/$defs/schema} alone.
     *
     * @param instance the {@code json_schema} half to validate
     */
    public List<String> validateSchemaHalf(final JsonNode instance) {
        return describe(this.schemaHalf.validate(toJson(instance), InputFormat.JSON));
    }

    private static String toJson(final JsonNode node) {
        try {
            return MAPPER.writeValueAsString(node);
        } catch (JsonProcessingException e) {
            throw new IllegalStateException("a parsed JSON tree could not be written back as JSON", e);
        }
    }

    // Fully qualified: an import would shadow java.lang.Error for the whole file.
    private static List<String> describe(final List<com.networknt.schema.Error> errors) {
        final List<String> described = new ArrayList<>(errors.size());
        for (final com.networknt.schema.Error error : errors) {
            described.add("  at `" + error.getInstanceLocation() + "`: " + error.getMessage());
        }
        return described;
    }
}
