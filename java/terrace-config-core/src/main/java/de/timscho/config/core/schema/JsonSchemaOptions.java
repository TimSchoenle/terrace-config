package de.timscho.config.core.schema;

/**
 * How {@link JsonSchemaRenderer} renders — ported from the Rust crate's {@code JsonSchema}
 * options type. Immutable; {@code with}-style methods return a new instance.
 */
public final class JsonSchemaOptions {

    /** The meta-schema URI for JSON Schema 2020-12 — what a current editor implements. */
    public static final String DRAFT_2020_12 = "https://json-schema.org/draft/2020-12/schema";

    /**
     * The meta-schema URI for JSON Schema draft-07 — what Helm validates {@code
     * values.schema.json} against. Nothing but the URI differs from 2020-12 for anything this
     * renderer emits.
     */
    public static final String DRAFT_07 = "http://json-schema.org/draft-07/schema#";

    private final String metaSchema;
    private final String id;
    private final String title;
    private final Docs docs;
    private final boolean defaults;
    private final boolean closed;
    private final boolean requirePresent;

    private JsonSchemaOptions(String metaSchema, String id, String title, Docs docs,
                               boolean defaults, boolean closed, boolean requirePresent) {
        this.metaSchema = metaSchema;
        this.id = id;
        this.title = title;
        this.docs = docs;
        this.defaults = defaults;
        this.closed = closed;
        this.requirePresent = requirePresent;
    }

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

    /** The dialect to declare. Defaults to {@link #DRAFT_2020_12}. */
    public JsonSchemaOptions withMetaSchema(String metaSchema) {
        return new JsonSchemaOptions(metaSchema, id, title, docs, defaults, closed, requirePresent);
    }

    /** The document's {@code $id}. Omitted by default. */
    public JsonSchemaOptions withId(String id) {
        return new JsonSchemaOptions(metaSchema, id, title, docs, defaults, closed, requirePresent);
    }

    /** The document's {@code title}. Omitted by default. */
    public JsonSchemaOptions withTitle(String title) {
        return new JsonSchemaOptions(metaSchema, id, title, docs, defaults, closed, requirePresent);
    }

    /** The document's {@code title}, unless one was already chosen. */
    public JsonSchemaOptions orTitle(String title) {
        return this.title != null ? this : withTitle(title);
    }

    /** How much of each key's documentation comment becomes its {@code description}. */
    public JsonSchemaOptions withDocs(Docs docs) {
        return new JsonSchemaOptions(metaSchema, id, title, docs, defaults, closed, requirePresent);
    }

    /** Whether each key carries its default as a {@code default} annotation. Defaults to {@code true}. */
    public JsonSchemaOptions withDefaults(boolean defaults) {
        return new JsonSchemaOptions(metaSchema, id, title, docs, defaults, closed, requirePresent);
    }

    /** Whether a key the configuration does not have is an error. Defaults to {@code true}. */
    public JsonSchemaOptions withClosed(boolean closed) {
        return new JsonSchemaOptions(metaSchema, id, title, docs, defaults, closed, requirePresent);
    }

    /**
     * Whether a key that must be supplied must be supplied <em>by this document</em>. Defaults
     * to {@code true}. Not the same question as {@link de.timscho.config.core.model.Key#isRequired()},
     * which is satisfied by any layer, this document included.
     */
    public JsonSchemaOptions withRequirePresent(boolean requirePresent) {
        return new JsonSchemaOptions(metaSchema, id, title, docs, defaults, closed, requirePresent);
    }

    public String metaSchema() {
        return metaSchema;
    }

    public String id() {
        return id;
    }

    public String title() {
        return title;
    }

    public Docs docs() {
        return docs;
    }

    public boolean defaults() {
        return defaults;
    }

    public boolean closed() {
        return closed;
    }

    public boolean requirePresent() {
        return requirePresent;
    }
}
