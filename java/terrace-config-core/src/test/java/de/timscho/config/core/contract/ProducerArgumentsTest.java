package de.timscho.config.core.contract;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.util.Set;
import org.junit.jupiter.api.Test;

/** The {@code --name value} command line every build-time producer entry point reads. */
class ProducerArgumentsTest {

    private static final Set<String> SINGLE = Set.of("type", "prefix");
    private static final Set<String> REPEATABLE = Set.of("reserve");

    private static ProducerArguments parse(final String... args) {
        return ProducerArguments.parse(args, SINGLE, REPEATABLE);
    }

    @Test
    void readsSingleAndRepeatedOptionsInTheOrderGiven() {
        ProducerArguments arguments =
                parse("--type", "a.B", "--reserve", "X_ONE", "--prefix", "X_", "--reserve", "X_TWO");

        assertThat(arguments.required("type")).isEqualTo("a.B");
        assertThat(arguments.required("prefix")).isEqualTo("X_");
        assertThat(arguments.all("reserve")).containsExactly("X_ONE", "X_TWO");
    }

    @Test
    void anAbsentOptionIsNullOrEmptyAndARequiredOneSaysWhichItIs() {
        ProducerArguments arguments = parse();

        assertThat(arguments.optional("type")).isNull();
        assertThat(arguments.all("reserve")).isEmpty();
        assertThatThrownBy(() -> arguments.required("type")).hasMessage("`--type` is required");
    }

    @Test
    void anEmptyRequiredValueIsAbsent() {
        assertThatThrownBy(() -> parse("--prefix", "").required("prefix")).hasMessage("`--prefix` is required");
    }

    @Test
    void aValueStartingWithDashesIsAValueNotAnOption() {
        assertThat(parse("--prefix", "--X_").required("prefix")).isEqualTo("--X_");
    }

    @Test
    void refusesAnUnknownOptionAMissingValueAndASingleOptionTwice() {
        assertThatThrownBy(() -> parse("--typo", "x")).hasMessage("unknown option `--typo`");
        assertThatThrownBy(() -> parse("--type")).hasMessage("`--type` needs a value");
        assertThatThrownBy(() -> parse("--type", "a", "--type", "b")).hasMessage("`--type` may be given only once");
        assertThatThrownBy(() -> parse("type", "a")).hasMessage("expected an option, found `type`");
        assertThatThrownBy(() -> parse("--", "a")).hasMessage("expected an option, found `--`");
    }
}
