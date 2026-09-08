package de.timscho.config.core.schema;

/**
 * How {@link TomlExampleRenderer} renders — ported from the Rust crate's {@code TomlExample}
 * options type. Immutable; {@code with}-style methods return a new instance.
 */
public final class TomlExampleOptions {

    /** The placeholder written in place of a secret's value. */
    private static final String SECRET = "<secret>";

    /** The placeholder written where a key has no default to show. */
    private static final String VALUE = "<value>";

    private final boolean header;
    private final Docs docs;
    private final boolean spellings;
    private final String secretPlaceholder;
    private final String placeholder;

    private TomlExampleOptions(boolean header, Docs docs, boolean spellings, String secretPlaceholder, String placeholder) {
        this.header = header;
        this.docs = docs;
        this.spellings = spellings;
        this.secretPlaceholder = secretPlaceholder;
        this.placeholder = placeholder;
    }

    /** The whole file: preamble, summaries, and every spelling. */
    public static TomlExampleOptions defaults() {
        return new TomlExampleOptions(true, Docs.SUMMARY, true, SECRET, VALUE);
    }

    /** Whether to emit the preamble. Defaults to {@code true}. */
    public TomlExampleOptions withHeader(boolean header) {
        return new TomlExampleOptions(header, docs, spellings, secretPlaceholder, placeholder);
    }

    /** How much of each key's comment to carry. Defaults to {@link Docs#SUMMARY}. */
    public TomlExampleOptions withDocs(Docs docs) {
        return new TomlExampleOptions(header, docs, spellings, secretPlaceholder, placeholder);
    }

    /** Whether to name the environment and file spellings above each key. Defaults to {@code true}. */
    public TomlExampleOptions withSpellings(boolean spellings) {
        return new TomlExampleOptions(header, docs, spellings, secretPlaceholder, placeholder);
    }

    /** What a secret key is written as. Defaults to {@code <secret>}. */
    public TomlExampleOptions withSecretPlaceholder(String secretPlaceholder) {
        return new TomlExampleOptions(header, docs, spellings, secretPlaceholder, placeholder);
    }

    /** What a key with no default is written as, when its type suggests nothing better. Defaults to {@code <value>}. */
    public TomlExampleOptions withPlaceholder(String placeholder) {
        return new TomlExampleOptions(header, docs, spellings, secretPlaceholder, placeholder);
    }

    public boolean header() {
        return header;
    }

    public Docs docs() {
        return docs;
    }

    public boolean spellings() {
        return spellings;
    }

    public String secretPlaceholder() {
        return secretPlaceholder;
    }

    public String placeholder() {
        return placeholder;
    }
}
