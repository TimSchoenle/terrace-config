package de.timscho.config.tck;

import static org.assertj.core.api.Assertions.assertThat;

import de.timscho.config.core.io.ContractCodec;
import de.timscho.config.core.model.Contract;
import de.timscho.config.tck.fixtures.FixtureContracts;
import de.timscho.config.tck.fixtures.SpringFixtureContracts;
import java.nio.file.Path;
import java.util.Map;
import java.util.function.Supplier;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.ValueSource;

/**
 * Holds every document the Java producers emit to {@code terrace-contract conform}, the one
 * implementation of {@code spec/v1/CONFORMANCE.md}, at the tier each producer claims: {@code
 * terrace-java} at tier 2, {@code spring-boot} at tier 1. The examples' Dockerfiles are held to
 * their contracts the same way, through {@code terrace-contract image verify}.
 *
 * <p>The meta-schema and tier checks in {@link JavaConformanceTest} and {@link
 * MetaSchemaValidatorTest} stay: they say which field differs, where {@code conform} says only
 * that something does. This class is what makes the claim the same one every other producer's
 * build makes. The shared corpus under {@code spec/v1/conformance/} is not repeated here: {@code
 * cli/tests/corpus.rs} already conforms it, and it is not a Java producer's output.
 *
 * <p>Needs the binary; see {@link TerraceContractBinary} for what happens without it.
 */
class TerraceContractConformTest {

    private static final Map<String, Supplier<Contract>> LOADER_CASES = Map.of(
            "minimal", FixtureContracts::minimal,
            "full-surface", FixtureContracts::fullSurface,
            "unnameable-key", FixtureContracts::unnameableKey,
            "required-entries", FixtureContracts::requiredEntries,
            "entry-names", FixtureContracts::entryNames,
            "element-patterns", FixtureContracts::elementPatterns);

    private static final Map<String, Supplier<Contract>> SPRING_CASES = Map.of(
            "minimal", SpringFixtureContracts::minimal,
            "full-surface", SpringFixtureContracts::fullSurface,
            "unnameable-key", SpringFixtureContracts::unnameableKey,
            "required-entries", SpringFixtureContracts::requiredEntries,
            "element-patterns", SpringFixtureContracts::elementPatterns);

    @ParameterizedTest
    @ValueSource(
            strings = {
                "minimal",
                "full-surface",
                "unnameable-key",
                "required-entries",
                "entry-names",
                "element-patterns"
            })
    void theLoadersEmissionMeetsTier2(final String caseName) {
        final byte[] document = ContractCodec.write(LOADER_CASES.get(caseName).get());

        assertConforms(TerraceContractBinary.require().conform(Tier.TIER_2, document), caseName);
    }

    @ParameterizedTest
    @ValueSource(strings = {"minimal", "full-surface", "unnameable-key", "required-entries", "element-patterns"})
    void springsEmissionMeetsTier1(final String caseName) {
        final byte[] document = ContractCodec.write(SPRING_CASES.get(caseName).get());

        assertConforms(TerraceContractBinary.require().conform(Tier.TIER_1, document), caseName);
    }

    /** The stored Java corpus: what the emissions above are byte-compared against at tier 3. */
    @ParameterizedTest
    @ValueSource(
            strings = {
                "minimal",
                "full-surface",
                "unnameable-key",
                "required-entries",
                "entry-names",
                "element-patterns"
            })
    void everyJavaGoldenMeetsTier2(final String caseName) {
        final Path golden = JavaGoldenPaths.goldenCase(caseName);

        assertConforms(TerraceContractBinary.require().conform(Tier.TIER_2, golden), golden.toString());
    }

    /**
     * The examples' committed contracts, which their builds' {@code terraceContractCheck} keeps
     * byte-equal to what {@code ContractMain} and {@code SpringContractMain} emit today.
     */
    @Test
    void theCommittedExampleContractsMeetTheirProducersTiers() {
        final TerraceContractCli cli = TerraceContractBinary.require();
        final Path java = SpecPaths.repositoryRoot().resolve("java");
        final Path loader = java.resolve("terrace-config-example-service/contract.json");
        final Path spring = java.resolve("terrace-config-example-spring-service/contract.json");

        assertConforms(cli.conform(Tier.TIER_2, loader), loader.toString());
        assertConforms(cli.conform(Tier.TIER_1, spring), spring.toString());
    }

    /**
     * Each example's Dockerfile carries the label block its committed contract renders, so an
     * image built from it says where its contract is and under which prefix.
     */
    @Test
    void theExampleDockerfilesCarryTheirContractsLabels() {
        final TerraceContractCli cli = TerraceContractBinary.require();
        final Path java = SpecPaths.repositoryRoot().resolve("java");
        for (final String example :
                new String[] {"terrace-config-example-service", "terrace-config-example-spring-service"}) {
            final Path module = java.resolve(example);
            final TerraceContractCli.Outcome outcome =
                    cli.verifyDockerfile(module.resolve("contract.json"), module.resolve("Dockerfile"));

            assertConforms(outcome, module.resolve("Dockerfile").toString());
        }
    }

    /**
     * Guards the checks above against passing vacuously: a document that meets tier 1 but spells
     * its dialect wrongly must be reported, with {@code conform}'s findings status.
     */
    @Test
    void aDocumentBelowTheTierIsReported() {
        final Path tierOneOnly = SpecPaths.conformanceCase("tier-1-spellings");

        final TerraceContractCli.Outcome outcome =
                TerraceContractBinary.require().conform(Tier.TIER_2, tierOneOnly);

        assertThat(outcome.exitCode()).as(outcome.output()).isEqualTo(1);
        assertThat(outcome.output()).isNotBlank();
    }

    private static void assertConforms(final TerraceContractCli.Outcome outcome, final String what) {
        assertThat(outcome.conforms())
                .as("terrace-contract conform over %s exited %d:%n%s", what, outcome.exitCode(), outcome.output())
                .isTrue();
    }
}
