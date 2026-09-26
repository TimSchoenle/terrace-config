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

    private static final String SLUG = "^[a-z0-9][a-z0-9_-]{0,63}$";

    @Test
    void anEntryNamePatternLandsInPropertyNamesAndRaisesTheVersion() {
        assertThat(described().getSchemaVersion()).isEqualTo(2);
        final Schema refined = described().refine(DOCUMENTS, Refinement.entryNames(SLUG));
        assertThat(documents(refined).getConstraint()).containsEntry("propertyNames", Map.of("pattern", SLUG));
        assertThat(Refiner.entryNamePattern(documents(refined))).isEqualTo(SLUG);
        assertThat(refined.getSchemaVersion()).isEqualTo(3);
        assertThat(described().refine(DOCUMENTS, imprintAndPrivacy()).getSchemaVersion())
                .isEqualTo(2);
    }

    @ParameterizedTest
    @CsvSource(
            delimiter = '|',
            value = {
                "^\\S+$|names a different set of characters",
                "^a.b$|excludes a different set of line terminators",
                "^[a-z]+?$|cannot itself be repeated or made lazy",
                "(?=a)|lookaround"
            })
    void aPatternOutsideThePortableSubsetIsRefusedNamingTheConstruct(final String pattern, final String reason) {
        assertThatThrownBy(() -> described().refine(DOCUMENTS, Refinement.entryNames(pattern)))
                .isInstanceOf(RefinementException.class)
                .hasMessageContaining("cannot be the entry-name pattern of `legal.documents`")
                .hasMessageContaining(reason)
                .hasMessageContaining("Portable patterns");
    }

    @Test
    void onePatternPerKeyAndTheSameOneTwiceChangesNothing() {
        final Schema once = described().refine(DOCUMENTS, Refinement.entryNames(SLUG));
        assertThat(once.refine(DOCUMENTS, Refinement.entryNames(SLUG))).isEqualTo(once);
        assertThatThrownBy(() -> once.refine(DOCUMENTS, Refinement.entryNames("^[a-z]+$")))
                .hasMessageContaining("already holds its entry names to");
    }

    @Test
    void aRequiredEntryThePatternRejectsIsRefusedInEitherOrder() {
        final Refinement letters = Refinement.entryNames("^[a-z]+$");
        final Refinement entries = Refinement.requiredEntries(List.of("imprint", "privacy2"));
        assertThatThrownBy(() -> described().refine(DOCUMENTS, letters).refine(DOCUMENTS, entries))
                .hasMessageContaining("`privacy2` cannot be a required entry of `legal.documents`")
                .hasMessageContaining("does not match");
        assertThatThrownBy(() -> described().refine(DOCUMENTS, entries).refine(DOCUMENTS, letters))
                .hasMessageContaining("requires the entry `privacy2`");
    }

    @Test
    void aDefaultNamingAnEntryThePatternRejectsIsNotADefaultInEitherOrder() {
        final Map<String, Object> capitalised = Map.of("Terms", "t");
        final Schema refineFirst =
                described().refine(DOCUMENTS, Refinement.entryNames(SLUG)).withDefaultsFromValue(defaults(capitalised));
        final Schema defaultsFirst =
                described().withDefaultsFromValue(defaults(capitalised)).refine(DOCUMENTS, Refinement.entryNames(SLUG));
        assertThat(refineFirst).isEqualTo(defaultsFirst);
        assertThat(documents(refineFirst).isRequired()).isTrue();
        assertThat(documents(refineFirst).getDefaultValue()).isNull();

        final Schema kept = described()
                .withDefaultsFromValue(defaults(Map.of("terms", "t")))
                .refine(DOCUMENTS, Refinement.entryNames(SLUG));
        assertThat(documents(kept).isRequired()).isFalse();
        assertThat(documents(kept).getDefaultValue()).isEqualTo(Map.of("terms", "t"));
    }

    @Test
    void theRenderingsSayWhatTheNamesMustMatch() {
        final Schema refined = described()
                .withDefaultsFromValue(defaults(Map.of()))
                .refine(DOCUMENTS, imprintAndPrivacy())
                .refine(DOCUMENTS, Refinement.entryNames(SLUG));
        assertThat(refined.toMarkdown())
                .contains("must contain: `imprint`, `privacy`, entry names match `" + SLUG + "`");
        assertThat(refined.toTomlExample()).contains("# Must contain: imprint, privacy\n# Entry names match: " + SLUG);
    }

    @Test
    void theRenderingsSayWhatTheMapMustContain() {
        final Schema refined =
                described().withDefaultsFromValue(defaults(Map.of())).refine(DOCUMENTS, imprintAndPrivacy());
        assertThat(refined.toMarkdown()).contains("must contain: `imprint`, `privacy`");
        assertThat(refined.toTomlExample()).contains("# Must contain: imprint, privacy");
    }
}
