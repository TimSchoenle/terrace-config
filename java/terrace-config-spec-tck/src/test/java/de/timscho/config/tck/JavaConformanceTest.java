package de.timscho.config.tck;

import static org.assertj.core.api.Assertions.assertThat;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import de.timscho.config.core.io.ContractCodec;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Producer;
import de.timscho.config.tck.fixtures.FixtureContracts;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.function.Supplier;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.ValueSource;

/**
 * Checks the Java-side rendering of the four named spec cases against every level {@code
 * spec/v1/CONFORMANCE.md} describes, from a real {@code @TerraceConfig} fixture (see {@code
 * fixtures/}) through the real loader and Jackson 2 codec — never a hand-written JSON literal
 * standing in for what the pipeline actually renders.
 *
 * <ul>
 *   <li><b>Meta-schema (tier 1)</b>: the produced envelope, and its {@code schema} half alone,
 *       both validate against {@code contract.schema.json} — see {@link MetaSchemaValidatorTest}
 *       for the same check run against the stored corpus itself.
 *   <li><b>Spec corpus (tier 2, cross-language)</b>: every dialect spelling — {@code env}, the
 *       file forms, every alias list, {@code unreachable} — agrees with the shared Rust-authored
 *       case in {@code spec/v1/conformance/}. Type names, the {@code json_schema} rendering and
 *       the {@code external} surface are exempt by design: {@code spec/v1/CONFORMANCE.md}
 *       reserves those for an implementation's own language, and forcing them to match would
 *       defeat the point of publishing a native one.
 *   <li><b>Java golden (tier 3, self-conformance)</b>: byte-identical, after {@code
 *       producer.version} is substituted, against a Java-only corpus this module keeps under
 *       {@code src/test/resources/conformance-java/}. Regenerate it with {@code ./gradlew
 *       :terrace-config-spec-tck:blessJavaConformance} (or {@code -Dterrace.spec.bless=true})
 *       after an intentional rendering change, then review the diff under {@code git diff} before
 *       committing it alongside that change — nothing else keeps Java's own output from drifting
 *       release to release the way {@code ContractCorpusRoundTripTest} (in {@code
 *       terrace-config-core-jackson2}/{@code -jackson3}) keeps it from drifting away from the
 *       corpus it already ships.
 * </ul>
 */
class JavaConformanceTest {

    private static final ObjectMapper MAPPER = new ObjectMapper();

    private static final Map<String, Supplier<Contract>> CASES = Map.of(
            "minimal", FixtureContracts::minimal,
            "full-surface", FixtureContracts::fullSurface,
            "unnameable-key", FixtureContracts::unnameableKey,
            "required-entries", FixtureContracts::requiredEntries,
            "entry-names", FixtureContracts::entryNames);

    @ParameterizedTest
    @ValueSource(strings = {"minimal", "full-surface", "unnameable-key", "required-entries", "entry-names"})
    void rendersAValidEnvelope(final String caseName) {
        final MetaSchemaValidator validator = MetaSchemaValidator.load();
        final JsonNode produced = producedNode(caseName);

        assertThat(validator.validateEnvelope(produced))
                .as("`%s`'s rendering against contract.schema.json", caseName)
                .isEmpty();
        assertThat(validator.validateSchemaHalf(produced.path("schema")))
                .as("`%s`'s schema half against #/$defs/schema", caseName)
                .isEmpty();
    }

    @ParameterizedTest
    @ValueSource(strings = {"minimal", "full-surface", "unnameable-key", "required-entries", "entry-names"})
    void meetsTier2AgainstTheSharedSpecCorpus(final String caseName) throws IOException {
        final JsonNode produced = producedNode(caseName);
        final JsonNode expected = MAPPER.readTree(Files.readAllBytes(SpecPaths.conformanceCase(caseName)));

        assertThat(TierComparator.compare(Tier.TIER_2, produced, expected))
                .as("`%s` against the tier 2 spec corpus", caseName)
                .isEmpty();
    }

    @ParameterizedTest
    @ValueSource(strings = {"minimal", "full-surface", "unnameable-key", "required-entries", "entry-names"})
    void meetsTier3AgainstItsOwnJavaGolden(final String caseName) throws IOException {
        final byte[] rendered =
                ContractCodec.write(withConformanceVersion(CASES.get(caseName).get()));
        final Path golden = JavaGoldenPaths.goldenCase(caseName);

        if (Boolean.getBoolean("terrace.spec.bless")) {
            Files.createDirectories(golden.getParent());
            Files.write(golden, rendered);
        }

        assertThat(Files.exists(golden))
                .as(
                        "no Java golden stored for `%s`; regenerate with "
                                + "./gradlew :terrace-config-spec-tck:blessJavaConformance",
                        caseName)
                .isTrue();
        assertThat(new String(rendered, StandardCharsets.UTF_8))
                .as(
                        "`%s` is stale against its Java golden; regenerate with "
                                + "./gradlew :terrace-config-spec-tck:blessJavaConformance",
                        caseName)
                .isEqualTo(Files.readString(golden, StandardCharsets.UTF_8));
    }

    /**
     * {@code required-entries} pins semantics tier 2's spellings do not reach: which keys a
     * refinement made required, which entries their constraints require, and which defaults it
     * dropped. Every producer has to agree on those, whatever its type vocabulary, or a chart gets a
     * different answer to "may I leave this out" depending on the language the image was written in.
     */
    @ParameterizedTest
    @ValueSource(strings = {"required-entries", "entry-names"})
    void publishesTheSameRefinementsAsTheSharedSpecCorpus(final String caseName) throws IOException {
        final JsonNode document = producedNode(caseName);
        final JsonNode corpus = MAPPER.readTree(Files.readAllBytes(SpecPaths.conformanceCase(caseName)));
        assertThat(document.path("schema").path("schema_version"))
                .as("`%s` schema_version", caseName)
                .isEqualTo(corpus.path("schema").path("schema_version"));
        final JsonNode produced = document.path("schema").path("keys");
        final JsonNode expected = corpus.path("schema").path("keys");

        assertThat(produced.size()).isEqualTo(expected.size());
        for (int i = 0; i < expected.size(); i++) {
            final JsonNode want = expected.get(i);
            final JsonNode got = produced.get(i);
            final String path = want.path("path").asText();
            assertThat(got.path("path").asText()).isEqualTo(path);
            assertThat(got.path("required")).as("`%s` required", path).isEqualTo(want.path("required"));
            assertThat(got.path("constraint").path("required"))
                    .as("`%s` constraint.required", path)
                    .isEqualTo(want.path("constraint").path("required"));
            assertThat(tightenings(got.path("constraint"), ""))
                    .as("`%s`'s tightenings, at every position inside it", path)
                    .isEqualTo(tightenings(want.path("constraint"), ""));
            assertThat(got.path("default_value").isNull())
                    .as("`%s` has a default exactly when the corpus does", path)
                    .isEqualTo(want.path("default_value").isNull());
        }
    }

    /**
     * Every tightening a constraint publishes, as {@code position keyword value} lines — the walk
     * {@code FORMAT.md} names, over the JSON both sides render, so the two element schemas may
     * differ everywhere else and still be compared where it matters.
     */
    private static List<String> tightenings(final JsonNode schema, final String at) {
        final List<String> found = new ArrayList<>();
        if (!schema.isObject()) {
            return found;
        }
        final boolean map = "object".equals(schema.path("type").asText()) && !schema.has("properties");
        if (map && schema.has("required")) {
            found.add(at + " required " + schema.get("required"));
        }
        if (map && schema.has("propertyNames")) {
            found.add(at + " propertyNames " + schema.get("propertyNames"));
        }
        if (schema.path("pattern").isTextual()) {
            found.add(at + " pattern " + schema.get("pattern"));
        }
        for (final String keyword : List.of("additionalProperties", "items")) {
            found.addAll(tightenings(schema.path(keyword), at + "/*"));
        }
        final JsonNode fields = schema.path("properties");
        for (final Map.Entry<String, JsonNode> field : fields.properties()) {
            found.addAll(tightenings(field.getValue(), at + "/" + field.getKey()));
        }
        return found;
    }

    private static JsonNode producedNode(final String caseName) {
        final byte[] rendered = ContractCodec.write(CASES.get(caseName).get());
        try {
            return MAPPER.readTree(rendered);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    /** {@code producer.version} substituted with {@link ConformanceVersion#VALUE} on a copy. */
    private static Contract withConformanceVersion(final Contract contract) {
        final Producer producer = contract.getProducer();
        return contract.toBuilder()
                .producer(Producer.builder()
                        .name(producer.getName())
                        .version(ConformanceVersion.VALUE)
                        .loader(producer.getLoader())
                        .build())
                .build();
    }
}
