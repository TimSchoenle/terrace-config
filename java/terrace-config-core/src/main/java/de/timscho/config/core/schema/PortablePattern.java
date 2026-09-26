package de.timscho.config.core.schema;

import java.util.regex.Pattern;
import java.util.regex.PatternSyntaxException;
import lombok.experimental.UtilityClass;
import org.jspecify.annotations.Nullable;

/**
 * The portable pattern subset — the port of the Rust crate's {@code schema::pattern}, and {@code
 * spec/v1/FORMAT.md}'s <em>Portable patterns</em>.
 *
 * <p>A pattern a {@link Refinement} publishes is read by whatever validates the contract: an
 * ECMA-262 engine behind a JSON Schema validator, Rust's {@code regex}, {@code java.util.regex},
 * Python's {@code re}. They disagree about {@code .}, {@code \d}, {@code \s}, {@code $} before a
 * trailing newline and set operations inside a class, so a refinement's pattern is held to the
 * constructs every one of them reads alike, and refused otherwise with the construct named. The
 * refusals are the Rust crate's, word for word.
 *
 * <p>{@link #compile} is how a pattern inside the subset is matched here. It is not {@link
 * Pattern#compile} on the source: Java's {@code $} also matches before a final line terminator, so
 * every unescaped {@code $} outside a class is given to Java as {@code \z}, the end of the text,
 * which is what the subset means by it.
 */
@UtilityClass
public class PortablePattern {

    /** The longest pattern accepted, in characters. */
    private static final int MAX_LENGTH = 4096;

    /** The largest count a repetition may name. */
    private static final int MAX_REPEAT = 1000;

    /** How deep groups may nest. */
    private static final int MAX_DEPTH = 32;

    /** The characters that mean something outside a class. */
    private static final String SYNTAX = "^$\\.*+?()[]{}|";

    /**
     * Why {@code source} lies outside the portable subset, or {@code null} when it lies inside.
     *
     * @param source the pattern
     */
    public static @Nullable String refusal(final String source) {
        if (source.isEmpty()) {
            return "an empty pattern matches every string, so publishing it states nothing";
        }
        final int length = source.codePointCount(0, source.length());
        if (length > MAX_LENGTH) {
            return "it is " + length + " characters long, beyond the " + MAX_LENGTH + " a pattern may be";
        }
        final Parser parser = new Parser(source.codePoints().toArray());
        try {
            parser.alternation();
            if (parser.peek() >= 0) {
                throw parser.refuse("`)` closes a group that was never opened; escape it as `\\)`");
            }
            return null;
        } catch (final Refused refused) {
            return refused.getMessage();
        }
    }

    /**
     * {@code source} compiled for matching, when it lies inside the subset; {@code null} otherwise.
     *
     * <p>Match with {@link java.util.regex.Matcher#find()}: a JSON Schema pattern is unanchored.
     *
     * @param source the pattern
     */
    public static @Nullable Pattern compile(final String source) {
        if (refusal(source) != null) {
            return null;
        }
        try {
            return Pattern.compile(forJava(source));
        } catch (final PatternSyntaxException unexpected) {
            return null;
        }
    }

    /** The subset's meaning, spelled for {@code java.util.regex}: {@code $} is the end of the text. */
    private static String forJava(final String source) {
        final StringBuilder out = new StringBuilder(source.length() + 4);
        boolean inClass = false;
        int index = 0;
        while (index < source.length()) {
            final char next = source.charAt(index);
            if (next == '\\' && index + 1 < source.length()) {
                out.append(next).append(source.charAt(index + 1));
                index += 2;
                continue;
            }
            if (inClass) {
                // The subset refuses an empty class, so the first `]` after `[` or `[^` closes it.
                if (next == ']') {
                    inClass = false;
                }
                out.append(next);
            } else if (next == '[') {
                inClass = true;
                out.append(next);
                if (index + 1 < source.length() && source.charAt(index + 1) == '^') {
                    out.append('^');
                    index++;
                }
            } else if (next == '$') {
                out.append("\\z");
            } else {
                out.append(next);
            }
            index++;
        }
        return out.toString();
    }

    /** A refusal, carried out of the recursion. */
    private static final class Refused extends Exception {
        private static final long serialVersionUID = 1L;

        Refused(final String message) {
            super(message, null, false, false);
        }
    }

    /** A recursive-descent reading of the subset, over code points. */
    private static final class Parser {
        private final int[] chars;
        private int at;
        private int depth;

        Parser(final int[] chars) {
            this.chars = chars;
        }

        int peek() {
            return peekAt(0);
        }

        int peekAt(final int offset) {
            return at + offset < chars.length ? chars[at + offset] : -1;
        }

        int bump() {
            final int next = peek();
            if (next >= 0) {
                at++;
            }
            return next;
        }

        boolean eat(final int expected) {
            if (peek() == expected) {
                at++;
                return true;
            }
            return false;
        }

        Refused refuse(final String why) {
            return new Refused("at character " + (at + 1) + ", " + why);
        }

        Refused refusePrevious(final String why) {
            return new Refused("at character " + at + ", " + why);
        }

        void alternation() throws Refused {
            do {
                sequence();
            } while (eat('|'));
        }

        void sequence() throws Refused {
            while (true) {
                final int next = peek();
                if (next < 0 || next == '|' || next == ')') {
                    return;
                }
                if (next == '^' || next == '$') {
                    bump();
                    if (isRepetition(peek())) {
                        throw refuse("an anchor cannot be repeated");
                    }
                } else {
                    atom();
                    repetition();
                }
            }
        }

        private static boolean isRepetition(final int next) {
            return next == '*' || next == '+' || next == '?' || next == '{';
        }

        void atom() throws Refused {
            final int next = bump();
            switch (next) {
                case '(' -> group();
                case '[' -> characterClass();
                case '\\' -> escape(false);
                case '.' ->
                    throw refusePrevious("`.` excludes a different set of line terminators in every engine; write "
                            + "the class of characters meant, such as `[^\\n]`");
                case '*', '+', '?' ->
                    throw refusePrevious("`" + Character.toString(next) + "` has nothing to repeat; escape it as `\\"
                            + Character.toString(next) + "` to mean the character");
                case '{' -> throw refusePrevious("`{` has nothing to repeat; escape it as `\\{` to mean the character");
                case ']', '}' ->
                    throw refusePrevious("`" + Character.toString(next)
                            + "` on its own is read differently by different engines; escape it as `\\"
                            + Character.toString(next) + "`");
                default -> withinThePlane(next);
            }
        }

        void group() throws Refused {
            if (peek() == '?') {
                if (peekAt(1) == ':') {
                    at += 2;
                } else {
                    throw refuse("only `(?:…)` may follow `(` with `?`; lookaround, named groups and flags are not "
                            + "read alike by every engine");
                }
            }
            if (depth >= MAX_DEPTH) {
                throw refuse("groups nest more than " + MAX_DEPTH + " deep");
            }
            depth++;
            alternation();
            depth--;
            if (!eat(')')) {
                throw refuse("a group is not closed; add the `)` it needs");
            }
        }

        void repetition() throws Refused {
            final int next = peek();
            if (next == '*' || next == '+' || next == '?') {
                bump();
            } else if (next == '{') {
                bump();
                counted();
            } else {
                return;
            }
            if (isRepetition(peek())) {
                throw refuse("a repetition cannot itself be repeated or made lazy; group what is meant with `(?:…)`");
            }
        }

        /** The rest of a counted repetition, its opening brace just consumed. */
        void counted() throws Refused {
            final int least = count();
            final int most;
            if (eat(',')) {
                most = peek() == '}' ? -1 : count();
            } else {
                most = least;
            }
            if (!eat('}')) {
                throw refuse("a counted repetition is `{n}`, `{n,}` or `{n,m}`; escape `{` as `\\{` to mean "
                        + "the character");
            }
            if (most >= 0 && most < least) {
                throw refusePrevious(
                        "`{" + least + "," + most + "}` asks for at least " + least + " and at most " + most);
            }
        }

        int count() throws Refused {
            final int start = at;
            long value = 0;
            while (peek() >= '0' && peek() <= '9') {
                value = Math.min(value * 10 + (bump() - '0'), Integer.MAX_VALUE);
            }
            if (at == start) {
                throw refuse("a counted repetition needs a number; escape `{` as `\\{` to mean the character");
            }
            if (value > MAX_REPEAT) {
                throw refusePrevious("a repetition counts at most " + MAX_REPEAT + ", and this asks for " + value);
            }
            return (int) value;
        }

        int escape(final boolean inClass) throws Refused {
            final int next = bump();
            if (next < 0) {
                throw refusePrevious("`\\` ends the pattern with nothing to escape");
            }
            if (next < 0x80 && SYNTAX.indexOf(next) >= 0) {
                return next;
            }
            return switch (next) {
                case '-' -> {
                    if (inClass) {
                        yield '-';
                    }
                    throw notAnEscape(next);
                }
                case 't' -> '\t';
                case 'n' -> '\n';
                case 'r' -> '\r';
                case 'f' -> '\f';
                case 'u' -> codePoint();
                case 'd', 'D', 'w', 'W', 's', 'S', 'b', 'B' ->
                    throw refusePrevious("`\\" + Character.toString(next)
                            + "` names a different set of characters in different engines — `\\s` holds U+FEFF in "
                            + "ECMA-262 and not in Rust, `\\d` is Unicode in Rust and ASCII in ECMA-262; write the "
                            + "class of characters meant");
                case '0', '1', '2', '3', '4', '5', '6', '7', '8', '9' ->
                    throw refusePrevious("a back-reference or octal escape is not read alike by every engine");
                default -> throw notAnEscape(next);
            };
        }

        private Refused notAnEscape(final int next) {
            return refusePrevious("`\\" + Character.toString(next)
                    + "` is not an escape every engine reads alike; the portable escapes are a syntax character, "
                    + "`\\t`, `\\n`, `\\r`, `\\f` and `\\uXXXX`");
        }

        int codePoint() throws Refused {
            int value = 0;
            for (int i = 0; i < 4; i++) {
                final int digit = Character.digit(peek(), 16);
                if (peek() < 0 || peek() >= 0x80 || digit < 0) {
                    throw refuse("`\\u` takes exactly four hex digits");
                }
                bump();
                value = value * 16 + digit;
            }
            if (value >= 0xD800 && value <= 0xDFFF) {
                throw refusePrevious("`\\u" + String.format("%04X", value)
                        + "` is a surrogate, which is half of a character rather than one");
            }
            return value;
        }

        void characterClass() throws Refused {
            eat('^');
            boolean first = true;
            while (true) {
                noSetDifference();
                final int low = classMember(first);
                if (low < 0) {
                    return;
                }
                first = false;
                noSetDifference();
                if (peek() == '-' && peekAt(1) != ']' && peekAt(1) >= 0) {
                    bump();
                    noSetDifference();
                    final int high = classMember(false);
                    if (high < 0) {
                        throw refusePrevious("a range needs an upper end");
                    }
                    if (high < low) {
                        throw refusePrevious("the range `" + describe(low) + "-" + describe(high) + "` runs backwards");
                    }
                }
            }
        }

        void noSetDifference() throws Refused {
            if (peek() == '-' && peekAt(1) == '-') {
                throw refuse("`--` inside a class is a set operation in Rust; escape the `-` meant as a character "
                        + "as `\\-`");
            }
        }

        /** One character of a class, or {@code -1} at the {@code ]} that closes it. */
        int classMember(final boolean first) throws Refused {
            final int next = bump();
            if (next < 0) {
                throw refusePrevious("a class is not closed; add the `]` it needs");
            }
            return switch (next) {
                case ']' -> closing(first);
                case '[' ->
                    throw refusePrevious(
                            "`[` inside a class opens a nested class in Rust and Java; escape it as `\\[`");
                case '\\' -> escape(true);
                case '-' -> dash(first);
                default -> literal(next);
            };
        }

        /** The {@code ]} closing a class, which cannot be its first member. */
        private int closing(final boolean first) throws Refused {
            if (first) {
                throw refusePrevious("an empty class matches nothing in ECMA-262 and is a literal `]` elsewhere; "
                        + "escape it as `\\]` to mean the character");
            }
            return -1;
        }

        /** An unescaped {@code -} inside a class, which stands for itself only first or last. */
        private int dash(final boolean first) throws Refused {
            if (first || peek() == ']') {
                return '-';
            }
            throw refusePrevious(
                    "a `-` inside a class is a range, or stands first or last; escape it as `\\-` " + "anywhere else");
        }

        /** Any other character inside a class. */
        private int literal(final int next) throws Refused {
            if ((next == '&' || next == '~' || next == '|') && peek() == next) {
                throw refusePrevious("`" + Character.toString(next) + Character.toString(next)
                        + "` inside a class is a set operation in Rust or Java; escape one of them");
            }
            withinThePlane(next);
            return next;
        }

        void withinThePlane(final int literal) throws Refused {
            if (literal >= 0xD800 && literal <= 0xDFFF) {
                throw refusePrevious("a lone surrogate is half of a character rather than one");
            }
            if (literal > 0xFFFF) {
                throw refusePrevious("`" + Character.toString(literal)
                        + "` lies beyond U+FFFF, which an ECMA-262 engine without the `u` flag reads as two "
                        + "characters");
            }
        }

        /** A character as Rust's {@code char::escape_default} writes it, so the messages match. */
        private static String describe(final int character) {
            return switch (character) {
                case '\t' -> "\\t";
                case '\n' -> "\\n";
                case '\r' -> "\\r";
                case '\\' -> "\\\\";
                case '\'' -> "\\'";
                case '"' -> "\\\"";
                default -> {
                    if (character >= 0x20 && character < 0x7F) {
                        yield Character.toString(character);
                    }
                    yield "\\u{" + Integer.toHexString(character) + "}";
                }
            };
        }
    }
}
