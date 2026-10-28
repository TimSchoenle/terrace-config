package de.timscho.config.core.contract;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import org.jspecify.annotations.Nullable;

/**
 * The parsed command line of a build-time producer entry point: {@code --name value} pairs, each
 * option either single-valued or repeatable.
 *
 * <p>Deliberately small rather than a dependency: the entry points run inside a service's own
 * build, on its own runtime classpath, where every library they bring is one more version the
 * service has to reconcile. Only the two-token {@code --name value} form is accepted, so a value
 * that itself starts with {@code --} cannot be mistaken for an option and no quoting rule is
 * needed.
 */
public final class ProducerArguments {

    private final Map<String, List<String>> values;

    private ProducerArguments(final Map<String, List<String>> values) {
        this.values = values;
    }

    /**
     * Parses {@code args} against the options a producer understands.
     *
     * @param args       the command line, as {@code main} received it
     * @param single     the options that may appear at most once, without their {@code --}
     * @param repeatable the options that may appear any number of times, without their {@code --}
     * @return the parsed options, by name
     * @throws IllegalArgumentException for an unknown option, a missing value, or a single-valued
     *                                  option given twice
     */
    public static ProducerArguments parse(final String[] args, final Set<String> single, final Set<String> repeatable) {
        final Map<String, List<String>> values = new LinkedHashMap<>();
        int index = 0;
        while (index < args.length) {
            final String token = args[index];
            if (!token.startsWith("--") || token.length() == 2) {
                throw new IllegalArgumentException("expected an option, found `" + token + "`");
            }
            final String name = token.substring(2);
            if (!single.contains(name) && !repeatable.contains(name)) {
                throw new IllegalArgumentException("unknown option `" + token + "`");
            }
            if (index + 1 >= args.length) {
                throw new IllegalArgumentException("`" + token + "` needs a value");
            }
            final List<String> seen = values.computeIfAbsent(name, ignored -> new ArrayList<>());
            if (!seen.isEmpty() && single.contains(name)) {
                throw new IllegalArgumentException("`" + token + "` may be given only once");
            }
            seen.add(args[index + 1]);
            index += 2;
        }
        return new ProducerArguments(values);
    }

    /**
     * {@return the value of a single-valued option that must be present and non-empty}
     *
     * @param name the option, without its {@code --}
     * @throws IllegalArgumentException when it is absent or empty
     */
    public String required(final String name) {
        final String value = this.optional(name);
        if (value == null || value.isEmpty()) {
            throw new IllegalArgumentException("`--" + name + "` is required");
        }
        return value;
    }

    /**
     * {@return the value of a single-valued option, or {@code null} when it was not given}
     *
     * @param name the option, without its {@code --}
     */
    public @Nullable String optional(final String name) {
        final List<String> given = this.values.get(name);
        return given == null ? null : given.get(0);
    }

    /**
     * {@return every value of a repeatable option, in the order given; empty when it was not given}
     *
     * @param name the option, without its {@code --}
     */
    public List<String> all(final String name) {
        final List<String> given = this.values.get(name);
        return given == null ? List.of() : List.copyOf(given);
    }
}
