package de.timscho.config.core.schema;

import lombok.AccessLevel;
import lombok.AllArgsConstructor;
import lombok.Value;
import lombok.With;
import lombok.experimental.Accessors;

/**
 * How {@link TomlExampleRenderer} renders — ported from the Rust crate's {@code TomlExample}
 * options type. Immutable; {@code with}-style methods return a new instance.
 */
@Value
@With
@AllArgsConstructor(access = AccessLevel.PRIVATE)
@Accessors(fluent = true)
public class TomlExampleOptions {

    /** The placeholder written in place of a secret's value. */
    private static final String SECRET = "<secret>";

    /** The placeholder written where a key has no default to show. */
    private static final String VALUE = "<value>";

    /** Whether to emit the preamble. Defaults to {@code true}. */
    boolean header;

    /** How much of each key's comment to carry. Defaults to {@link Docs#SUMMARY}. */
    Docs docs;

    /** Whether to name the environment and file spellings above each key. Defaults to {@code true}. */
    boolean spellings;

    /** What a secret key is written as. Defaults to {@code <secret>}. */
    String secretPlaceholder;

    /** What a key with no default is written as, when its type suggests nothing better. Defaults to {@code <value>}. */
    String placeholder;

    /** The whole file: preamble, summaries, and every spelling. */
    public static TomlExampleOptions defaults() {
        return new TomlExampleOptions(true, Docs.SUMMARY, true, SECRET, VALUE);
    }
}
