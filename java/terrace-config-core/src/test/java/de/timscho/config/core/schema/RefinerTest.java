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
        assertThat(Refiner.tightenings(refined))
                .containsExactly(new Refiner.Entries("", List.of("imprint", "privacy")));
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
        assertThat(Refiner.tightenings(documents(refined))).containsExactly(new Refiner.Names("", SLUG));
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

    private static final String LOCALE = "^[A-Za-z]{2,3}(?:[-_][A-Za-z]{4})?(?:[-_](?:[A-Za-z]{2}|[0-9]{3}))?$";

    /** terrace-legal's shape, by hand: every chapter a map of locale to text. */
    private static Schema chapters() {
        final Map<String, Object> chapter = constraint(
                "type",
                "object",
                "properties",
                constraint(
                        "body", constraint("type", "object", "additionalProperties", constraint("type", "string")),
                        "title", constraint("type", "object", "additionalProperties", constraint("type", "string"))));
        return Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(
                        key("chapters", constraint("type", "object", "additionalProperties", chapter)),
                        key("locale", constraint("type", "string")),
                        key("tags", constraint("type", "array", "items", constraint("type", "string")))))
                .build();
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> body(final Schema schema) {
        final Map<String, Object> chapter =
                (Map<String, Object>) schema.getKeys().get(0).getConstraint().get("additionalProperties");
        return (Map<String, Object>) ((Map<String, Object>) chapter.get("properties")).get("body");
    }

    @Test
    void aPathContinuesIntoTheElementAndItsFields() {
        final Schema refined = chapters()
                .refine("chapters.*.body", Refinement.entryNames(LOCALE))
                .refine("chapters.*.body.*", Refinement.nonBlank())
                .refine("chapters.*.title", Refinement.requiredEntries(List.of("en")))
                .refine("locale", Refinement.pattern(LOCALE))
                .refine("tags.*", Refinement.nonBlank());
        assertThat(body(refined)).containsEntry("propertyNames", Map.of("pattern", LOCALE));
        assertThat(((Map<?, ?>) body(refined).get("additionalProperties")).get("pattern"))
                .isEqualTo(Refinement.NON_BLANK);
        assertThat(refined.getSchemaVersion()).isEqualTo(3);
        assertThat(Refiner.tightenings(refined.getKeys().get(0)))
                .containsExactly(
                        new Refiner.Names("*.body", LOCALE),
                        new Refiner.Matches("*.body.*", Refinement.NON_BLANK),
                        new Refiner.Entries("*.title", List.of("en")));
        // The schema it was given is untouched: refining copies what it changes.
        assertThat(body(chapters())).doesNotContainKey("propertyNames");
        assertThat(chapters().refine("locale", Refinement.pattern(LOCALE)).getSchemaVersion())
                .isEqualTo(2);
    }

    @ParameterizedTest
    @CsvSource(
            delimiter = '|',
            value = {
                "chapters.*.bodyy|`bodyy` is not a field of `chapters.*`, so `chapters.*.bodyy` names nothing. Its "
                        + "fields are `body`, `title`.",
                "chapters.intro.body|`chapters` is a map, and `intro` would be one entry of it",
                "locale.*|`locale` is neither a map nor a sequence",
                "chapters..body|has an empty segment",
                "chapters|`chapters` is not a string",
                "tags.*.x|`tags.*` is not a struct, so it has no field `x`"
            })
    void aPositionTheTypeDidNotDescribeIsRefusedWithTheReason(final String path, final String reason) {
        assertThatThrownBy(() -> chapters().refine(path, Refinement.nonBlank()))
                .isInstanceOf(RefinementException.class)
                .hasMessageContaining(reason);
    }

    @Test
    void aDefaultANestedRefinementRejectsIsNotADefaultInEitherOrder() {
        final Map<String, Object> blank =
                Map.of("chapters", Map.of("intro", Map.of("body", Map.of("en", " \u3000 "))), "locale", "en");
        final Schema refineFirst =
                chapters().refine("chapters.*.body.*", Refinement.nonBlank()).withDefaultsFromValue(blank);
        final Schema defaultsFirst =
                chapters().withDefaultsFromValue(blank).refine("chapters.*.body.*", Refinement.nonBlank());
        assertThat(refineFirst).isEqualTo(defaultsFirst);
        assertThat(refineFirst.getKeys().get(0).isRequired()).isTrue();

        final Map<String, Object> feff =
                Map.of("chapters", Map.of("intro", Map.of("body", Map.of("en", "\uFEFF"))), "locale", "en");
        assertThat(chapters()
                        .withDefaultsFromValue(feff)
                        .refine("chapters.*.body.*", Refinement.nonBlank())
                        .getKeys()
                        .get(0)
                        .isRequired())
                .isFalse();
    }

    @Test
    void aSecondPatternIsRefusedAndOneNoChoiceMatchesIsUnsatisfiable() {
        final Schema once = chapters().refine("locale", Refinement.pattern(LOCALE));
        assertThat(once.refine("locale", Refinement.pattern(LOCALE))).isEqualTo(once);
        assertThatThrownBy(() -> once.refine("locale", Refinement.nonBlank()))
                .hasMessageContaining("is already matched against");

        final Schema choice = Schema.builder()
                .schemaVersion(2)
                .dialect(dialect())
                .keys(List.of(key("level", constraint("type", "string", "enum", List.of("info", "warn")))))
                .build();
        assertThatThrownBy(() -> choice.refine("level", Refinement.pattern("^x")))
                .hasMessageContaining("matches none of them");
        assertThat(choice.refine("level", Refinement.pattern("^i"))
                        .getKeys()
                        .get(0)
                        .getConstraint())
                .containsEntry("pattern", "^i");
    }

    @Test
    void nonBlankIsExactlyWhatTrimLeavesNonEmptyForEveryCharacter() {
        final java.util.regex.Pattern blank = PortablePattern.compile(Refinement.NON_BLANK);
        assertThat(blank).isNotNull();
        for (int code = 0; code <= Character.MAX_CODE_POINT; code++) {
            if (code >= 0xD800 && code <= 0xDFFF) {
                continue;
            }
            // Rust's `char::is_whitespace` is Unicode's White_Space property.
            final boolean whiteSpace = isWhiteSpace(code);
            assertThat(blank.matcher(Character.toString(code)).find())
                    .as("U+%04X", code)
                    .isEqualTo(!whiteSpace);
        }
    }

    /** Unicode's {@code White_Space} property, which {@link Character#isWhitespace} is not. */
    private static boolean isWhiteSpace(final int code) {
        return (code >= 0x09 && code <= 0x0D)
                || code == 0x20
                || code == 0x85
                || code == 0xA0
                || code == 0x1680
                || (code >= 0x2000 && code <= 0x200A)
                || code == 0x2028
                || code == 0x2029
                || code == 0x202F
                || code == 0x205F
                || code == 0x3000;
    }

    @Test
    void theRenderingsSayWhereInsideTheKeyEachRefinementIs() {
        final Schema refined = chapters()
                .refine("chapters.*.body", Refinement.entryNames("^[a-z]{2}$"))
                .refine("locale", Refinement.pattern("^[a-z]{2}$"));
        assertThat(refined.toMarkdown())
                .contains("at `*.body`: entry names match `^[a-z]{2}$`")
                .contains("matches `^[a-z]{2}$`");
        assertThat(refined.toTomlExample())
                .contains("# Entry names match at *.body: ^[a-z]{2}$")
                .contains("# Matches: ^[a-z]{2}$");
    }

    @Test
    void theRenderingsSayWhatTheMapMustContain() {
        final Schema refined =
                described().withDefaultsFromValue(defaults(Map.of())).refine(DOCUMENTS, imprintAndPrivacy());
        assertThat(refined.toMarkdown()).contains("must contain: `imprint`, `privacy`");
        assertThat(refined.toTomlExample()).contains("# Must contain: imprint, privacy");
    }
}
