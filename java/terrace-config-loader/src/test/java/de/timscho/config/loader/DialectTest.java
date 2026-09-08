package de.timscho.config.loader;

import static org.assertj.core.api.Assertions.assertThat;

import java.util.Map;

import org.junit.jupiter.api.Test;

class DialectTest {

    @Test
    void a_key_round_trips_between_its_two_spellings() {
        Dialect dialect = Dialect.of("TEST_");
        assertThat(dialect.keyPath("AUTH__JWT_SECRET")).isEqualTo("auth.jwt_secret");
        assertThat(dialect.envSpelling("auth.jwt_secret")).isEqualTo("TEST_AUTH__JWT_SECRET");
    }

    @Test
    void a_custom_separator_changes_where_nesting_happens() {
        Dialect dialect = Dialect.of("TEST_").withNestingSeparator("_");
        assertThat(dialect.keyPath("AUTH_JWT")).isEqualTo("auth.jwt");
        assertThat(dialect.envSpelling("auth.jwt")).isEqualTo("TEST_AUTH_JWT");
    }

    @Test
    void reserving_is_by_full_environment_spelling() {
        Dialect dialect = Dialect.of("TEST_").reserve("TEST_PROFILE");
        assertThat(dialect.isReserved("TEST_PROFILE")).isTrue();
        assertThat(dialect.isReserved("PROFILE")).isFalse();
        assertThat(dialect.isReserved("TEST_OTHER")).isFalse();
    }

    @Test
    void a_separator_containing_a_letter_round_trips() {
        Dialect dialect = Dialect.of("TEST_").withNestingSeparator("_X_");
        assertThat(dialect.keyPath("AUTH_X_JWT")).isEqualTo("auth.jwt");
        assertThat(dialect.envSpelling("auth.jwt")).isEqualTo("TEST_AUTH_X_JWT");
    }

    @Test
    void a_separator_containing_a_letter_matches_in_either_case() {
        Dialect dialect = Dialect.of("TEST_").withNestingSeparator("_X_");
        for (String spelling : new String[] {"AUTH_X_JWT", "auth_x_jwt", "Auth_X_jwt"}) {
            assertThat(dialect.keyPath(spelling)).as(spelling).isEqualTo("auth.jwt");
        }
    }

    @Test
    void a_name_that_is_only_the_prefix_and_the_suffix_is_not_an_indirection() {
        Dialect dialect = Dialect.of("TEST_");
        assertThat(dialect.indirectionTarget("TEST_FILE")).isEmpty();
        assertThat(dialect.indirectionTarget("TEST_AUTH_FILE")).hasValue("AUTH");
        assertThat(dialect.indirectionTarget("TEST_PROFILE")).isEmpty();
        assertThat(dialect.indirectionTarget("OTHER_AUTH_FILE")).isEmpty();
    }

    @Test
    void reserving_ignores_case_in_both_directions() {
        Dialect upper = Dialect.of("TEST_").reserve("TEST_PROFILE");
        assertThat(upper.isReserved("TEST_profile")).isTrue();
        assertThat(upper.isReserved("test_Profile")).isTrue();

        Dialect lower = Dialect.of("TEST_").reserve("test_profile");
        assertThat(lower.isReserved("TEST_PROFILE")).isTrue();
    }

    @Test
    void a_key_named_for_the_suffix_is_still_seen_by_the_shadow_check() {
        Dialect dialect = Dialect.of("TEST_");
        Map<String, String> env = Map.of("TEST_FILE", "value");
        assertThat(dialect.plainEnvKeys(env)).contains("file");
    }
}
