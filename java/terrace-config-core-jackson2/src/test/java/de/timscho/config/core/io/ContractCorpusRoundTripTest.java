package de.timscho.config.core.io;

import static org.assertj.core.api.Assertions.assertThat;

import de.timscho.config.core.model.Contract;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.MethodSource;

/**
 * The corpus round trip, independent of any Java producer: every stored {@code contract.json} must
 * deserialise into {@link Contract} and re-serialise to <em>exactly</em> the same bytes.
 *
 * <p>This is the strongest check the model has, and it needs no producer to run: it exercises the
 * whole model and every field-ordering decision at once, and it fails the moment the model cannot
 * represent something the format allows. Mirrors the intent of {@code rust/tests/spec.rs} and
 * {@code MetaSchemaValidatorTest} in {@code terrace-config-spec-tck}, one directory below.
 */
class ContractCorpusRoundTripTest {

    /**
     * {@code java/terrace-config-core} is two levels below the repository root, the same depth as
     * {@code java/terrace-config-spec-tck}; see that module's {@code SpecPaths}.
     */
    private static Path conformanceDir() {
        return Paths.get("")
                .toAbsolutePath()
                .resolve("../../spec/v1/conformance")
                .normalize();
    }

    static List<String> corpusCases() throws IOException {
        List<String> cases = new ArrayList<>();
        try (DirectoryStream<Path> entries = Files.newDirectoryStream(conformanceDir(), Files::isDirectory)) {
            for (Path entry : entries) {
                cases.add(entry.getFileName().toString());
            }
        }
        cases.sort(String::compareTo);
        return cases;
    }

    @ParameterizedTest
    @MethodSource("corpusCases")
    void roundTripsByte(String caseName) throws IOException {
        Path storedFile = conformanceDir().resolve(caseName).resolve("contract.json");
        byte[] stored = Files.readAllBytes(storedFile);

        Contract contract = ContractCodec.read(stored);
        byte[] rendered = ContractCodec.write(contract);

        assertThat(new String(rendered, StandardCharsets.UTF_8))
                .as("re-serialising `%s` must reproduce the stored bytes exactly", caseName)
                .isEqualTo(new String(stored, StandardCharsets.UTF_8));
    }

    /** The file forms read and write the same bytes as the in-memory forms. */
    @ParameterizedTest
    @MethodSource("corpusCases")
    void roundTripsThroughAFile(String caseName, @TempDir Path dir) throws IOException {
        Path storedFile = conformanceDir().resolve(caseName).resolve("contract.json");
        Path written = dir.resolve("contract.json");

        ContractCodec.write(ContractCodec.read(storedFile), written);

        assertThat(Files.readString(written))
                .as("writing `%s` read from its file must reproduce the stored bytes exactly", caseName)
                .isEqualTo(Files.readString(storedFile));
    }

    @Test
    void discoversAtLeastTheThreeKnownCases() throws IOException {
        assertThat(corpusCases())
                .contains(
                        "minimal",
                        "full-surface",
                        "unnameable-key",
                        "required-entries",
                        "entry-names",
                        "element-patterns",
                        "conditions");
    }
}
