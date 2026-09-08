package de.timscho.config.tck;

import java.io.IOException;
import java.nio.file.Files;
import java.util.List;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.ValueSource;

import static org.assertj.core.api.Assertions.assertThat;

/**
 * Proved without any producer, exactly as {@code rust/tests/spec.rs} proves it for the Rust
 * crate: the stored corpus itself is checked against {@code spec/v1/contract.schema.json}. A case
 * that no longer validates is the one failure a corpus alone cannot catch — an expectation blessed
 * while wrong.
 */
class MetaSchemaValidatorTest {

    private static final ObjectMapper MAPPER = new ObjectMapper();

    @Test
    void the_meta_schema_itself_compiles() {
        // Compiling it is the check: a meta-schema with a dangling $ref or a misspelt keyword
        // validates everything silently, which is worse than validating nothing.
        MetaSchemaValidator.load();
    }

    @ParameterizedTest
    @ValueSource(strings = {"minimal", "full-surface", "unnameable-key"})
    void every_stored_case_satisfies_the_meta_schema(String name) throws IOException {
        MetaSchemaValidator validator = MetaSchemaValidator.load();
        JsonNode contract = readCase(name);

        List<String> errors = validator.validateEnvelope(contract);
        assertThat(errors)
                .as("the stored `%s` case against contract.schema.json", name)
                .isEmpty();
    }

    @ParameterizedTest
    @ValueSource(strings = {"minimal", "full-surface", "unnameable-key"})
    void the_schema_half_of_each_case_satisfies_the_meta_schema_on_its_own(String name) throws IOException {
        // The `json` rendering is published as an artefact of its own, so the subschema has to be
        // reachable and correct without the envelope around it.
        MetaSchemaValidator validator = MetaSchemaValidator.load();
        JsonNode schemaHalf = readCase(name).path("schema");

        List<String> errors = validator.validateSchemaHalf(schemaHalf);
        assertThat(errors)
                .as("the `%s` case's schema half against #/$defs/schema", name)
                .isEmpty();
    }

    private static JsonNode readCase(String name) throws IOException {
        return MAPPER.readTree(Files.readString(SpecPaths.conformanceCase(name)));
    }
}
