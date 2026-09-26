package de.timscho.config.core.schema;

import static org.assertj.core.api.Assertions.assertThat;

import java.util.regex.Pattern;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.CsvSource;
import org.junit.jupiter.params.provider.ValueSource;

class PortablePatternTest {

    @ParameterizedTest
    @ValueSource(
            strings = {
                "^[a-z0-9][a-z0-9_-]{0,63}$",
                "^[A-Za-z]{2,3}(?:[-_][A-Za-z]{4})?(?:[-_](?:[A-Za-z]{2}|[0-9]{3}))?$",
                "[^\\t\\n\\u000B\\f\\r \\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000]",
                "^(a|b|)$",
                "^[-a]$",
                "^[a-]$",
                "^x{3}y{1,}z{0,1000}$"
            })
    void thePortableIdiomsAreAcceptedAndCompile(final String pattern) {
        assertThat(PortablePattern.refusal(pattern)).isNull();
        assertThat(PortablePattern.compile(pattern)).isNotNull();
    }

    @ParameterizedTest
    @CsvSource(
            delimiter = '|',
            value = {
                "a.b|at character 2, `.`",
                "\\s|`\\s`",
                "\\d|`\\d`",
                "(a)\\1|back-reference",
                "\\x41|`\\x`",
                "(?=a)|lookaround",
                "a*?|lazy",
                "a{2,1}|at most 1",
                "a{1001}|at most 1000",
                "*a|nothing to repeat",
                "^*|anchor",
                "a)|never opened",
                "(a|not closed",
                "[]a]|empty class",
                "[a[b]]|nested class",
                "[a-b-c]|`-`",
                "[z-a]|backwards",
                "[a&&b]|set operation",
                "[a--b]|set operation",
                "\\uD800|surrogate",
                "\\u12|four hex digits"
            })
    void everyConstructOutsideTheSubsetIsRefusedWithItsPosition(final String pattern, final String reason) {
        assertThat(PortablePattern.refusal(pattern)).contains(reason);
        assertThat(PortablePattern.compile(pattern)).isNull();
    }

    @Test
    void anEmptyPatternAndOneBeyondThePlaneAreRefused() {
        assertThat(PortablePattern.refusal("")).contains("an empty pattern");
        assertThat(PortablePattern.refusal("\uD83D\uDE00")).contains("beyond U+FFFF");
    }

    @Test
    void dollarIsTheEndOfTheTextAsInEcmaScriptAndNotBeforeAFinalNewline() {
        final Pattern slug = PortablePattern.compile("^[a-z0-9][a-z0-9_-]{0,63}$");
        assertThat(slug).isNotNull();
        assertThat(slug.matcher("terms").find()).isTrue();
        assertThat(slug.matcher("Terms").find()).isFalse();
        // `java.util.regex` alone would accept this; the subset's `$` does not.
        assertThat(slug.matcher("terms\n").find()).isFalse();
        // A `$` inside a class stays the character.
        final Pattern dollar = PortablePattern.compile("^[$]$");
        assertThat(dollar).isNotNull();
        assertThat(dollar.matcher("$").find()).isTrue();
    }
}
