package de.timscho.config.tck;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.MethodSource;

/**
 * Proves {@code terrace-config-core-jackson2} and {@code terrace-config-core-jackson3} are
 * byte-for-byte interchangeable: for every document in the shared spec corpus and this module's
 * own Java golden corpus, reading it and writing it straight back through each backend's {@code
 * ContractCodec} produces the exact same bytes in both.
 *
 * <p>Run through two isolated classloaders (see {@link IsolatedContractCodec}) rather than as a
 * normal test dependency on both modules: they publish a class of the identical name {@code
 * de.timscho.config.core.io.ContractCodec}, so this test's own classpath can carry at most one of
 * them at a time. {@code build.gradle.kts}'s {@code jackson2Runtime}/{@code jackson3Runtime}
 * configurations resolve each backend's own runtime classpath — jar and transitive dependencies —
 * into the {@code terrace.tck.jackson2Classpath}/{@code terrace.tck.jackson3Classpath} system
 * properties this test reads.
 */
class JacksonParityTest {

    static List<Path> corpusDocuments() throws IOException {
        final List<Path> documents = new ArrayList<>();
        collectInto(SpecPaths.specV1Dir().resolve("conformance"), documents);
        collectInto(JavaGoldenPaths.conformanceJavaDir(), documents);
        documents.sort(Comparator.comparing(Path::toString));
        return documents;
    }

    private static void collectInto(final Path corpusDir, final List<Path> out) throws IOException {
        try (DirectoryStream<Path> entries = Files.newDirectoryStream(corpusDir, Files::isDirectory)) {
            for (Path entry : entries) {
                final Path document = entry.resolve("contract.json");
                if (Files.exists(document)) {
                    out.add(document);
                }
            }
        }
    }

    @ParameterizedTest
    @MethodSource("corpusDocuments")
    void jackson2AndJackson3RoundTripTheSameBytes(final Path document) throws IOException {
        final byte[] stored = Files.readAllBytes(document);

        final byte[] jackson2Result;
        try (IsolatedContractCodec jackson2 =
                IsolatedContractCodec.forClasspath(requireProperty("terrace.tck.jackson2Classpath"))) {
            jackson2Result = jackson2.roundTrip(stored);
        }
        final byte[] jackson3Result;
        try (IsolatedContractCodec jackson3 =
                IsolatedContractCodec.forClasspath(requireProperty("terrace.tck.jackson3Classpath"))) {
            jackson3Result = jackson3.roundTrip(stored);
        }

        assertThat(new String(jackson3Result, StandardCharsets.UTF_8))
                .as("%s: Jackson 3's rendering must match Jackson 2's exactly", document)
                .isEqualTo(new String(jackson2Result, StandardCharsets.UTF_8));
    }

    private static String requireProperty(final String name) {
        final String value = System.getProperty(name);
        if (value == null || value.isBlank()) {
            throw new IllegalStateException(
                    name + " is not set; run through Gradle's `test` task, which resolves it from this "
                            + "module's jackson2Runtime/jackson3Runtime configurations");
        }
        return value;
    }
}
