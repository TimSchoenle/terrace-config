package de.timscho.config.core.schema;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Schema;
import de.timscho.config.core.model.TextForm;
import de.timscho.config.core.refusal.ConstraintEvaluator;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import org.junit.jupiter.api.Test;

/** {@link Refinement.Holds}, pinned to the Rust crate's encodings and sentences word for word. */
class ConditionTest {

    private static Map<String, Object> map(final Object... pairs) {
        final Map<String, Object> schema = new TreeMap<>();
        for (int index = 0; index < pairs.length; index += 2) {
            schema.put((String) pairs[index], pairs[index + 1]);
        }
        return schema;
    }

    /** terrace-legal's document, by hand: hosted or external, with a consent struct. */
    private static Schema documents() {
        final Map<String, Object> consent = map(
                "type",
                "object",
                "properties",
                map(
                        "requirement", map("type", "string", "enum", List.of("none", "accept")),
                        "version", map("type", "string"),
                        "grace_days", map("type", "integer", "minimum", 0, "maximum", 365),
                        "effective", map("type", "string")));
        final Map<String, Object> document = map(
                "type",
                "object",
                "properties",
                map(
                        "body", map("type", "object", "additionalProperties", map("type", "string")),
                        "url", map("type", "string"),
                        "consent", consent));
        return Schema.builder()
                .schemaVersion(2)
                .dialect(Dialect.builder()
                        .prefix("SITE_")
                        .nestingSeparator("__")
                        .indirectionSuffix("_FILE")
                        .build())
                .keys(List.of(Key.builder()
                        .path("documents")
                        .env("SITE_DOCUMENTS")
                        .constraint(map("type", "object", "additionalProperties", document))
                        .textForm(TextForm.STRUCTURED)
                        .build()))
                .build();
    }

    private static Refinement hostedOrExternal() {
        return Refinement.holds(Condition.exactlyOne(Condition.present("url"), Condition.nonEmpty("body")));
    }

    private static Refinement consentRule() {
        return Refinement.holds(Condition.when(
                Condition.notEqualTo("consent.requirement", "none"),
                Condition.all(
                        Condition.absent("url"),
                        Condition.matches("consent.version", "[^ ]"),
                        Condition.when(
                                Condition.above("consent.grace_days", 0), Condition.present("consent.effective")))));
    }

    @SuppressWarnings("unchecked")
    private static List<Object> conditions(final Schema schema) {
        final Map<String, Object> element =
                (Map<String, Object>) schema.getKeys().get(0).getConstraint().get("additionalProperties");
        return (List<Object>) element.get("allOf");
    }

    @Test
    void exactlyOneOfHostedAndExternalIsPublishedWithItsSentence() {
        final Schema refined = documents().refine("documents.*", hostedOrExternal());
        assertThat(conditions(refined))
                .containsExactly(map(
                        "description",
                        "exactly one of: [`url` is set; `body` is not empty]",
                        "oneOf",
                        List.of(
                                map("required", List.of("url")),
                                map("required", List.of("body"), "properties", map("body", map("minProperties", 1))))));
        assertThat(refined.getSchemaVersion()).isEqualTo(4);
        assertThat(refined.refine("documents.*", hostedOrExternal())).isEqualTo(refined);
        assertThat(Refiner.tightenings(refined.getKeys().get(0)))
                .containsExactly(new Refiner.Holds("*", "exactly one of: [`url` is set; `body` is not empty]"));
    }

    @Test
    void aConsentRuleNestsItsGraceRuleInsideItsConsequence() {
        final Schema refined = documents().refine("documents.*", consentRule());
        final Map<?, ?> member = (Map<?, ?>) conditions(refined).get(0);
        assertThat(member.get("description"))
                .isEqualTo("when `consent.requirement` is set and not \"none\": all of: [`url` is not set; "
                        + "`consent.version` matches `[^ ]`; when `consent.grace_days` is above 0: "
                        + "`consent.effective` is set]");
        assertThat(member.get("if"))
                .isEqualTo(map(
                        "required",
                        List.of("consent"),
                        "properties",
                        map(
                                "consent",
                                map(
                                        "required",
                                        List.of("requirement"),
                                        "properties",
                                        map("requirement", map("not", map("const", "none")))))));
    }

    @Test
    void theEvaluatorHoldsADocumentToItsConditionsAndSaysTheRule() {
        final Schema refined =
                documents().refine("documents.*", hostedOrExternal()).refine("documents.*", consentRule());
        final Map<String, Object> constraint = refined.getKeys().get(0).getConstraint();
        final Map<String, Object> hosted = map("body", map("en", "x"));
        final Map<String, Object> both = map("body", map("en", "x"), "url", "u");

        assertThat(ConstraintEvaluator.verdict(constraint, map("terms", hosted)))
                .isInstanceOf(ConstraintEvaluator.Holds.class);
        assertThat(ConstraintEvaluator.verdict(constraint, map("terms", both)))
                .isEqualTo(new ConstraintEvaluator.Fails(
                        "`terms` does not satisfy: exactly one of: [`url` is set; `body` is not empty]"));
        final Map<String, Object> graceWithoutDate =
                map("body", map("en", "x"), "consent", map("requirement", "accept", "version", "1", "grace_days", 5));
        assertThat(ConstraintEvaluator.verdict(constraint, map("terms", graceWithoutDate)))
                .isInstanceOf(ConstraintEvaluator.Fails.class);
        final Map<String, Object> noRequirement =
                map("body", map("en", "x"), "consent", map("requirement", "none", "grace_days", 5));
        assertThat(ConstraintEvaluator.verdict(constraint, map("terms", noRequirement)))
                .isInstanceOf(ConstraintEvaluator.Holds.class);
    }

    @Test
    void aConditionNamesOnlyDeclaredFieldsOnAStruct() {
        assertThatThrownBy(() -> documents().refine("documents", hostedOrExternal()))
                .hasMessageContaining("`documents` is not a struct, so it has no fields for a condition to relate");
        assertThatThrownBy(() -> documents().refine("documents.*", Refinement.holds(Condition.present("urll"))))
                .hasMessageContaining("`urll` is not a field of `documents.*`, so a condition on `urll` names nothing. "
                        + "Its fields are `body`, `consent`, `url`.");
        assertThatThrownBy(() -> documents()
                        .refine("documents.*", Refinement.holds(Condition.equalTo("consent.requirement", "always"))))
                .hasMessageContaining("`consent.requirement` can never be \"always\"");
        assertThatThrownBy(() -> documents().refine("documents.*", Refinement.holds(Condition.above("url", 0))))
                .hasMessageContaining("`url` is not a number");
        assertThatThrownBy(() -> documents()
                        .refine("documents.*", Refinement.holds(Condition.exactlyOne(Condition.present("url")))))
                .hasMessageContaining("at least two conditions");
    }

    @Test
    void theRenderingsSayEachConditionInItsOwnWords() {
        final Schema refined = documents().refine("documents.*", hostedOrExternal());
        assertThat(refined.toMarkdown()).contains("at `*`: exactly one of: [`url` is set; `body` is not empty]");
        assertThat(refined.toTomlExample())
                .contains("# Holds at *: exactly one of: [`url` is set; `body` is not empty]");
    }

    @Test
    void anUnsetFieldInADefaultTableIsAbsentNotNull() {
        final Map<String, Object> observed = new TreeMap<>();
        observed.put("url", null);
        observed.put("body", map("en", "x"));
        final Schema defaulted = documents().withDefaultsFromValue(map("documents", map("terms", observed)));
        assertThat(defaulted.getKeys().get(0).getDefaultValue()).isEqualTo(map("terms", map("body", map("en", "x"))));
    }
}
