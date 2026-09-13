package de.timscho.config.core.io;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;

import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.MethodSource;

import de.timscho.config.core.model.Contract;
import de.timscho.config.core.schema.JsonSchemaOptions;

/**
 * Proves {@code Schema.toJsonSchemaWith} against the stored corpus: every {@code contract.json}
 * already carries the exact document the Rust producer rendered for its {@code schema} half, in
 * its own {@code json_schema} field, so re-rendering from the deserialised {@link
 * de.timscho.config.core.model.Schema} and comparing structurally — not byte for byte, since
 * {@code java.util.Map} carries no key order of its own — is the strongest check available
 * without a Java producer of its own.
 */
class JsonSchemaRendererCorpusTest {

    static List<String> corpusCases() throws IOException {
        return ContractCorpusRoundTripTest.corpusCases();
    }

    @ParameterizedTest
    @MethodSource("corpusCases")
    void rendersTheSameDocumentTheStoredContractCarries(String caseName) throws IOException {
        Path storedFile = conformanceDir().resolve(caseName).resolve("contract.json");
        Contract contract = ContractCodec.read(Files.readAllBytes(storedFile));

        JsonSchemaOptions options =
                JsonSchemaOptions.forContract().orTitle(contract.getApp().getName() + " configuration");
        Map<String, Object> rendered = contract.getSchema().toJsonSchemaWith(options);

        assertThat(rendered)
                .as("re-rendering `%s`'s schema must reproduce the stored `json_schema` field", caseName)
                .isEqualTo(contract.getJsonSchema().asMap());
    }

    private static Path conformanceDir() {
        return java.nio.file.Paths.get("")
                .toAbsolutePath()
                .resolve("../../spec/v1/conformance")
                .normalize();
    }
}
