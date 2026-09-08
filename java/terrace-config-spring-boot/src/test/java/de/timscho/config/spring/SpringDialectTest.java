package de.timscho.config.spring;

import java.util.Optional;

import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;

class SpringDialectTest {

    private final SpringDialect dialect = SpringDialect.standard();

    @Test
    void a_file_suffixed_name_names_its_own_target() {
        assertThat(dialect.indirectionTarget("MYAPP_GITHUB_TOKEN_FILE"))
                .contains("MYAPP_GITHUB_TOKEN");
        assertThat(dialect.isIndirection("MYAPP_GITHUB_TOKEN_FILE")).isTrue();
    }

    @Test
    void a_name_that_is_only_the_suffix_names_no_target() {
        assertThat(dialect.indirectionTarget("_FILE")).isEmpty();
        assertThat(dialect.isIndirection("_FILE")).isFalse();
    }

    @Test
    void a_name_without_the_suffix_is_not_an_indirection() {
        assertThat(dialect.indirectionTarget("MYAPP_GITHUB_TOKEN")).isEmpty();
        assertThat(dialect.isIndirection("MYAPP_GITHUB_TOKEN")).isFalse();
    }

    @Test
    void a_name_ending_in_the_suffix_by_coincidence_still_counts() {
        // Deliberately documenting the behaviour, not endorsing it: this dialect has no way to
        // tell a field genuinely named `profile` from `_FILE`'s own suffix appearing as a
        // coincidence of spelling. The vanilla loader's `_FILE` suffix has the same property.
        Optional<String> target = dialect.indirectionTarget("MYAPP_PRO_FILE");
        assertThat(target).contains("MYAPP_PRO");
    }

    @Test
    void property_name_folds_case_and_replaces_the_separator_with_a_dot() {
        assertThat(dialect.propertyName("MYAPP_GITHUB_TOKEN")).isEqualTo("myapp.github.token");
    }

    @Test
    void the_single_underscore_separator_cannot_distinguish_nesting_from_a_literal_underscore() {
        // The divergence from `terrace-config-loader`'s `__` separator, made concrete: a field
        // genuinely named `github_token` and a nested key path `github.token` are both spelled
        // `MYAPP_GITHUB_TOKEN` in the environment, so both fold to the identical property name
        // here — an ambiguity `__` never has, since it never appears inside one segment's name.
        assertThat(dialect.propertyName("MYAPP_GITHUB_TOKEN")).isEqualTo("myapp.github.token");
    }

    @Test
    void the_separator_and_suffix_can_be_replaced() {
        SpringDialect custom = dialect.withNestingSeparator("__").withIndirectionSuffix("_PATH");
        assertThat(custom.separator()).isEqualTo("__");
        assertThat(custom.indirectionSuffix()).isEqualTo("_PATH");
        assertThat(custom.indirectionTarget("MYAPP__GITHUB__TOKEN_PATH"))
                .contains("MYAPP__GITHUB__TOKEN");
        assertThat(custom.propertyName("MYAPP__GITHUB__TOKEN")).isEqualTo("myapp.github.token");
    }
}
