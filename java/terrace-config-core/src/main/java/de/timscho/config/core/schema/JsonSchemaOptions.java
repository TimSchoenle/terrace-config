package de.timscho.config.core.schema;

import lombok.AccessLevel;
import lombok.AllArgsConstructor;
import lombok.Value;
import lombok.With;
import lombok.experimental.Accessors;

/**
 * How {@link JsonSchemaRenderer} renders — ported from the Rust crate's {@code JsonSchema}
 * options type. Immutable; {@code with}-style methods return a new instance.
 */
@Value
@With
@AllArgsConstructor(access = AccessLevel.PRIVATE)
@Accessors(fluent = true)
public class JsonSchemaOptions {

    /** The meta-schema URI for JSON Schema 2020-12 — what a current editor implements. */
    public static final String DRAFT_2020_12 = "https://json-schema.org/draft/2020-12/schema";

    /**
     * The meta-schema URI for JSON Schema draft-07 — what Helm validates {@code
     * values.schema.json} against. Nothing but the URI differs from 2020-12 for anything this
     * renderer emits.
     */
    public static final String DRAFT_07 = "http://json-schema.org/draft-07/schema#";

    /** The dialect to declare. Defaults to {@link #DRAFT_2020_12}. */
    String metaSchema;

    /** The document's {@code $id}. Omitted by default. */
    String id;

    /** The document's {@code title}. Omitted by default. */
    String title;

    /** How much of each key's documentation comment becomes its {@code description}. */
    Docs docs;

    /** Whether each key carries its default as a {@code default} annotation. Defaults to {@code true}. */
    boolean defaults;

    /** Whether a key the configuration does not have is an error. Defaults to {@code true}. */
    boolean closed;

    /**
     * Whether a key that must be supplied must be supplied <em>by this document</em>. Defaults
     * to {@code true}. Not the same question as {@link de.timscho.config.core.model.Key#isRequired()},
     * which is satisfied by any layer, this document included.
     */
    boolean requirePresent;

    /** JSON Schema 2020-12, closed, with every comment and every default. */
    public static JsonSchemaOptions standard() {
        return new JsonSchemaOptions(DRAFT_2020_12, null, null, Docs.FULL, true, true, true);
    }

    /**
     * The options a {@code Contract}'s embedded {@code json_schema} half uses: {@link
     * #DRAFT_07} (what Helm validates {@code values.schema.json} against), closed, and {@code
     * requirePresent} off, since the reader checking a rendered document is not looking at the
     * only layer that could have supplied a key — the environment or a mounted file are just as
     * valid.
     */
    public static JsonSchemaOptions forContract() {
        return standard().withMetaSchema(DRAFT_07).withRequirePresent(false);
    }

    /** The document's {@code title}, unless one was already chosen. */
    public JsonSchemaOptions orTitle(String title) {
        return this.title != null ? this : withTitle(title);
    }
}
