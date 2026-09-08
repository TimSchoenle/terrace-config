package de.timscho.config.core.io;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.MethodSource;

import de.timscho.config.core.model.Contract;

import static org.assertj.core.api.Assertions.assertThat;

/**
 * The Jackson-3 twin of {@code terrace-config-core-jackson2}'s test of the same name: every
 * stored {@code contract.json} must deserialise into {@link Contract} and re-serialise to
 * <em>exactly</em> the same bytes, this time through {@code tools.jackson.databind}. Proves the
 * shared, dual-annotated model (see {@code java/lombok.config}) really does work under both
 * Jackson majors, not just the one exercised first.
 */
class ContractCorpusRoundTripTest {

    /**
     * {@code java/terrace-config-core-jackson3} is two levels below the repository root, the
     * same depth as {@code java/terrace-config-spec-tck} — see that module's {@code SpecPaths}.
     */
    private static Path conformanceDir() {
        return Paths.get("").toAbsolutePath().resolve("../../spec/v1/conformance").normalize();
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

    @Test
    void discoversAtLeastTheThreeKnownCases() throws IOException {
        assertThat(corpusCases()).contains("minimal", "full-surface", "unnameable-key");
    }
}
