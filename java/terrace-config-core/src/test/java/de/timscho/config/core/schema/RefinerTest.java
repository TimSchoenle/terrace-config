package de.timscho.config.core.schema;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.TextForm;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.CsvSource;

/** Every rule {@link Refiner} documents, pinned the way the Rust crate's {@code tests/refine.rs} pins it. */
class RefinerTest {

    private static final String DOCUMENTS = "legal.documents";

    private static Dialect dialect() {
        return Dialect.builder()
                .prefix("PORTFOLIO_")
                .nestingSeparator("__")
                .indirectionSuffix("_FILE")
                .build();
    }

    private static Map<String, Object> constraint(final Object... pairs) {
        final Map<String, Object> schema = new TreeMap<>();
        for (int i = 0; i < pairs.length; i += 2) {
            schema.put((String) pairs[i], pairs[i + 1]);
        }
        return schema;
    }

    private static Key key(final String path, final Map<String, Object> constraint) {
        final String env =
                "PORTFOLIO_" + path.toUpperCase(java.util.Locale.ROOT).replace(".", "__");
        return Key.builder()
                .path(path)
                .env(env)
                .envFile(env + "_FILE")
                .secretsFile(path.replace(".", "__"))
                .constraint(constraint)
                .textForm(TextForm.STRUCTURED)
                .aliases(path.equals(DOCUMENTS) ? List.of("legal.docs") : List.of())
                .build();
    }

    private static Schema described() {
        return Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(
                        key(DOCUMENTS, constraint("type", "object")),
                        key(
                                "legal.links",
                                constraint("type", "object", "additionalProperties", constraint("type", "string"))),
                        key("legal.title", constraint("type", "string"))))
                .build();
    }

    private static Map<String, Object> defaults(final Map<String, Object> documents) {
        return Map.of("legal", Map.of("documents", documents, "links", Map.of(), "title", ""));
    }

    private static Refinement imprintAndPrivacy() {
        return Refinement.requiredEntries(List.of("privacy", "imprint"));
    }

    private static Key documents(final Schema schema) {
        return schema.getKeys().get(0);
    }

    @Test
    void publishesTheEntriesSortedInsideTheConstraintAndLeavesTheMapOpen() {
        final Key refined = documents(described().refine(DOCUMENTS, imprintAndPrivacy()));
        assertThat(refined.getConstraint()).containsEntry("required", List.of("imprint", "privacy"));
        assertThat(refined.getConstraint()).containsEntry("type", "object");
        assertThat(refined.getConstraint()).doesNotContainKey("additionalProperties");
        assertThat(Refiner.requiredEntries(refined)).containsExactly("imprint", "privacy");
    }

    @Test
    void aDefaultTheRefinementRejectsMakesTheKeyRequiredWithNoDefault() {
        final Schema defaulted = described().withDefaultsFromValue(defaults(Map.of()));
        assertThat(documents(defaulted).getDefaultValue()).isEqualTo(Map.of());

        final Key refined = documents(defaulted.refine(DOCUMENTS, imprintAndPrivacy()));
        assertThat(refined.isRequired()).isTrue();
        assertThat(refined.getDefaultText()).isNull();
        assertThat(refined.getDefaultValue()).isNull();
    }

    @Test
    void aKeyWithNoDefaultKeepsItsRequiredFlag() {
        assertThat(documents(described().refine(DOCUMENTS, imprintAndPrivacy())).isRequired())
                .isFalse();
    }

    @Test
    void refiningUnionsIsIdempotentAndNeverLoosens() {
        final Schema once = described()
                .refine(DOCUMENTS, Refinement.requiredEntries(List.of("privacy")))
                .refine(DOCUMENTS, imprintAndPrivacy());
        assertThat(documents(once).getConstraint()).containsEntry("required", List.of("imprint", "privacy"));
        assertThat(once.refine(DOCUMENTS, imprintAndPrivacy())).isEqualTo(once);
        assertThat(once.refine(DOCUMENTS, Refinement.requiredEntries(List.of("imprint"))))
                .isEqualTo(once);
    }

    @Test
    void refiningAndObservingDefaultsCommute() {
        final Map<String, Object> both = Map.of("imprint", "i", "privacy", "p");
        for (final Map<String, Object> observed : List.of(Map.<String, Object>of(), both)) {
            final Schema refineFirst =
                    described().refine(DOCUMENTS, imprintAndPrivacy()).withDefaultsFromValue(defaults(observed));
            final Schema defaultsFirst =
                    described().withDefaultsFromValue(defaults(observed)).refine(DOCUMENTS, imprintAndPrivacy());
            assertThat(refineFirst).isEqualTo(defaultsFirst);
        }
    }

    @Test
    void anEmptySetChangesNothingButIsStillChecked() {
        final Refinement empty = Refinement.requiredEntries(List.of());
        assertThat(described().refine(DOCUMENTS, empty)).isEqualTo(described());
        assertThatThrownBy(() -> described().refine("legal.documnets", empty))
                .isInstanceOf(RefinementException.class)
                .hasMessageContaining("`legal.documnets` is not a key");
        assertThatThrownBy(() -> described().refine("legal.title", empty))
                .isInstanceOf(RefinementException.class)
                .hasMessageContaining("is not a map");
    }

    @Test
    void anUnknownPathSaysWhatItIsInstead() {
        assertThatThrownBy(() -> described().refine("legal.docs", imprintAndPrivacy()))
                .hasMessageContaining("alias of `legal.documents`");
        assertThatThrownBy(() -> described().refine("legal", imprintAndPrivacy()))
                .hasMessageContaining("It is a table");
    }

    @Test
    void aClosedMapOrAKeyWithNoConstraintIsRefused() {
        final Schema closed = described().toBuilder()
                .keys(List.of(key(DOCUMENTS, constraint("type", "object", "additionalProperties", false))))
                .build();
        assertThatThrownBy(() -> closed.refine(DOCUMENTS, imprintAndPrivacy())).hasMessageContaining("is not a map");

        final Schema bare =
                described().toBuilder().keys(List.of(key(DOCUMENTS, null))).build();
        assertThatThrownBy(() -> bare.refine(DOCUMENTS, imprintAndPrivacy()))
                .hasMessageContaining("publishes no constraint");
    }

    @ParameterizedTest
    @CsvSource(
            delimiter = '|',
            value = {
                "Imprint|reads that as `legal.documents.imprint`",
                "a.b|reads `.` as nesting",
                "terms__v2|reads that as `legal.documents.terms.v2`",
                "imprint_file|is read as the `_FILE` indirection"
            })
    void anEntryNameNoLayerCanSpellIsRefusedWithTheReason(final String entry, final String reason) {
        assertThatThrownBy(() -> described().refine(DOCUMENTS, Refinement.requiredEntries(List.of(entry))))
                .isInstanceOf(RefinementException.class)
                .hasMessageContaining("`" + entry + "` cannot be a required entry of `legal.documents`")
                .hasMessageContaining(reason);
    }

    @Test
    void anEmptyEntryNameIsRefused() {
        assertThatThrownBy(() -> described().refine(DOCUMENTS, Refinement.requiredEntries(List.of(""))))
                .hasMessageContaining("an empty name");
    }

    @Test
    void aLibraryRefinesRelativeToWhereTheHostMountsIt() {
        final Schema direct = described().refine(DOCUMENTS, imprintAndPrivacy());
        final Refine library = () -> List.of(new Refine.At("documents", imprintAndPrivacy()));
        final Refine whole = () -> List.of(new Refine.At("", imprintAndPrivacy()));

        assertThat(described().refineWith("legal", library)).isEqualTo(direct);
        assertThat(described().refineWith(DOCUMENTS, whole)).isEqualTo(direct);
        assertThatThrownBy(() -> described().refineWith("site", library)).hasMessageContaining("`site.documents`");
    }

    @Test
    void aKeyMadeRequiredMakesItsTablesRequiredAndCarriesItsEntries() {
        final Map<String, Object> rendered = described()
                .withDefaultsFromValue(defaults(Map.of()))
                .refine(DOCUMENTS, imprintAndPrivacy())
                .toJsonSchema();
        assertThat(rendered).containsEntry("required", List.of("legal"));
        final Map<?, ?> legal = (Map<?, ?>) ((Map<?, ?>) rendered.get("properties")).get("legal");
        final Map<?, ?> documents = (Map<?, ?>) ((Map<?, ?>) legal.get("properties")).get("documents");
        assertThat(documents.get("required")).isEqualTo(List.of("imprint", "privacy"));
    }

    @Test
    void theRenderingsSayWhatTheMapMustContain() {
        final Schema refined =
                described().withDefaultsFromValue(defaults(Map.of())).refine(DOCUMENTS, imprintAndPrivacy());
        assertThat(refined.toMarkdown()).contains("must contain: `imprint`, `privacy`");
        assertThat(refined.toTomlExample()).contains("# Must contain: imprint, privacy");
    }
}
