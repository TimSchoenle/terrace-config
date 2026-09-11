package de.timscho.config.tck;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.IOException;
import java.nio.file.Files;
import java.util.List;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.ValueSource;

/**
 * Exercised against the stored corpus rather than against a producer, which does not exist yet
 * (PR 6, PR 7): a document compared with itself must satisfy every tier, and a document mutated in
 * one recorded way must fail at exactly the tier that field belongs to. That is what proves the
 * comparator's own logic before there is a rendering to run it on for real.
 */
class TierComparatorTest {

    private static final ObjectMapper MAPPER = new ObjectMapper();

    @ParameterizedTest
    @ValueSource(strings = {"minimal", "full-surface", "unnameable-key"})
    void a_document_compared_with_itself_satisfies_every_tier(String name) throws IOException {
        JsonNode contract = readCase(name);

        for (Tier tier : Tier.values()) {
            List<String> diffs = TierComparator.compare(tier, contract, contract);
            assertThat(diffs).as("`%s` against itself at %s", name, tier).isEmpty();
        }
    }

    @Test
    void tier_3_reports_a_field_that_no_longer_matches() throws IOException {
        JsonNode expected = readCase("minimal");
        JsonNode produced = expected.deepCopy();
        ((ObjectNode) produced.path("schema").path("keys").get(0)).put("default", "changed");

        List<String> diffs = TierComparator.compare(Tier.TIER_3, produced, expected);

        assertThat(diffs).hasSize(1);
        assertThat(diffs.getFirst()).contains("default").contains("changed").contains("public");
    }

    @Test
    void tier_2_reports_an_unreachable_mismatch_without_needing_byte_equality() throws IOException {
        JsonNode expected = readCase("unnameable-key");
        JsonNode produced = expected.deepCopy();
        ObjectNode firstKey = (ObjectNode) produced.path("schema").path("keys").get(0);
        // Whatever the stored value is, flip it, so the test does not depend on which key happens
        // to be first once the corpus changes.
        String original = firstKey.path("unreachable").asText(null);
        firstKey.put("unreachable", original == null ? "unnameable" : "different-from-stored");

        List<String> diffs = TierComparator.compare(Tier.TIER_2, produced, expected);

        assertThat(diffs).isNotEmpty();
        assertThat(diffs).anySatisfy(diff -> assertThat(diff).contains("unreachable"));
    }

    @Test
    void tier_1_reports_a_blank_producer_field() throws IOException {
        JsonNode contract = readCase("minimal");
        JsonNode produced = contract.deepCopy();
        ((ObjectNode) produced.path("producer")).put("loader", "");

        List<String> diffs = TierComparator.compare(Tier.TIER_1, produced, contract);

        assertThat(diffs).anySatisfy(diff -> assertThat(diff).contains("producer.loader"));
    }

    private static JsonNode readCase(String name) throws IOException {
        return MAPPER.readTree(Files.readString(SpecPaths.conformanceCase(name)));
    }
}
