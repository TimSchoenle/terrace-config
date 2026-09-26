package de.timscho.config.core.schema;

import java.util.List;
import java.util.Objects;
import org.jspecify.annotations.Nullable;

/**
 * One condition on the fields of a struct — the port of the Rust crate's {@code schema::Condition}.
 *
 * <p>A field is named by its path relative to the struct the condition is stated on, dotted through
 * nested structs: {@code url}, or {@code consent.requirement}. "Set" means present in the document.
 * {@link Refinement#holds} publishes one; {@link Refiner} checks every field it names against the
 * struct and every predicate against the field's type, and renders the same sentence the Rust crate
 * does.
 */
public sealed interface Condition
        permits Condition.Present,
                Condition.Absent,
                Condition.NonEmpty,
                Condition.Empty,
                Condition.Equals,
                Condition.NotEquals,
                Condition.Above,
                Condition.Matches,
                Condition.All,
                Condition.ExactlyOne,
                Condition.When {

    /**
     * The field is set.
     *
     * @param field the field's path
     */
    record Present(String field) implements Condition {}

    /**
     * The field is not set.
     *
     * @param field the field's path
     */
    record Absent(String field) implements Condition {}

    /**
     * The field is set and not empty: a map or struct with an entry, a sequence with an item, a
     * string with a character.
     *
     * @param field the field's path
     */
    record NonEmpty(String field) implements Condition {}

    /**
     * The field is not set, or set and empty.
     *
     * @param field the field's path
     */
    record Empty(String field) implements Condition {}

    /**
     * The field is set and equal to the value.
     *
     * @param field the field's path
     * @param value the value, as a producer holds a JSON value
     */
    record Equals(String field, @Nullable Object value) implements Condition {}

    /**
     * The field is set and not equal to the value.
     *
     * @param field the field's path
     * @param value the value, as a producer holds a JSON value
     */
    record NotEquals(String field, @Nullable Object value) implements Condition {}

    /**
     * The field is set and a number strictly above the bound.
     *
     * @param field the field's path
     * @param bound the bound
     */
    record Above(String field, Number bound) implements Condition {}

    /**
     * The field is set and a string matching the pattern, inside the portable subset.
     *
     * @param field   the field's path
     * @param pattern the pattern
     */
    record Matches(String field, String pattern) implements Condition {}

    /**
     * Every condition holds.
     *
     * @param conditions the conditions
     */
    record All(List<Condition> conditions) implements Condition {

        /**
         * Copies {@code conditions}.
         *
         * @param conditions the conditions
         */
        public All {
            conditions = List.copyOf(conditions);
        }
    }

    /**
     * Exactly one condition holds.
     *
     * @param conditions the conditions, at least two
     */
    record ExactlyOne(List<Condition> conditions) implements Condition {

        /**
         * Copies {@code conditions}.
         *
         * @param conditions the conditions
         */
        public ExactlyOne {
            conditions = List.copyOf(conditions);
        }
    }

    /**
     * Whenever {@code condition} holds, so does {@code then}.
     *
     * @param condition the condition
     * @param then      what it requires
     */
    record When(Condition condition, Condition then) implements Condition {

        /**
         * Refuses a missing half.
         *
         * @param condition the condition
         * @param then      what it requires
         */
        public When {
            Objects.requireNonNull(condition, "condition");
            Objects.requireNonNull(then, "then");
        }
    }

    /**
     * {@link Present}.
     *
     * @param field the field's path
     * @return the condition
     */
    static Condition present(final String field) {
        return new Present(field);
    }

    /**
     * {@link Absent}.
     *
     * @param field the field's path
     * @return the condition
     */
    static Condition absent(final String field) {
        return new Absent(field);
    }

    /**
     * {@link NonEmpty}.
     *
     * @param field the field's path
     * @return the condition
     */
    static Condition nonEmpty(final String field) {
        return new NonEmpty(field);
    }

    /**
     * {@link Empty}.
     *
     * @param field the field's path
     * @return the condition
     */
    static Condition empty(final String field) {
        return new Empty(field);
    }

    /**
     * {@link Equals}.
     *
     * @param field the field's path
     * @param value the value
     * @return the condition
     */
    static Condition equalTo(final String field, @Nullable final Object value) {
        return new Equals(field, value);
    }

    /**
     * {@link NotEquals}.
     *
     * @param field the field's path
     * @param value the value
     * @return the condition
     */
    static Condition notEqualTo(final String field, @Nullable final Object value) {
        return new NotEquals(field, value);
    }

    /**
     * {@link Above}.
     *
     * @param field the field's path
     * @param bound the bound
     * @return the condition
     */
    static Condition above(final String field, final Number bound) {
        return new Above(field, bound);
    }

    /**
     * {@link Matches}.
     *
     * @param field   the field's path
     * @param pattern the pattern
     * @return the condition
     */
    static Condition matches(final String field, final String pattern) {
        return new Matches(field, pattern);
    }

    /**
     * {@link All}.
     *
     * @param conditions the conditions
     * @return the condition
     */
    static Condition all(final Condition... conditions) {
        return new All(List.of(conditions));
    }

    /**
     * {@link ExactlyOne}.
     *
     * @param conditions the conditions
     * @return the condition
     */
    static Condition exactlyOne(final Condition... conditions) {
        return new ExactlyOne(List.of(conditions));
    }

    /**
     * {@link When}.
     *
     * @param condition the condition
     * @param then      what it requires
     * @return the condition
     */
    static Condition when(final Condition condition, final Condition then) {
        return new When(condition, then);
    }
}
