package de.timscho.config.core.model;

import java.util.List;
import java.util.Map;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import lombok.Builder;
import lombok.NonNull;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;

/**
 * One configuration key, in every spelling that can supply it — {@code
 * spec/v1/contract.schema.json}'s {@code #/$defs/key}.
 *
 * <p>The fields on {@code key.required} in the meta-schema ({@link #path} through
 * {@link #reserved}, minus {@link #unreachable}) are marked {@link JsonInclude.Include#ALWAYS}
 * below so that, for example, a {@code null} {@link #ty} is still rendered rather than omitted —
 * the meta-schema's own {@code "type": ["string", "null"]} depends on the field being present.
 * {@link #constraint}, {@link #textConstraint} and {@link #unreachable} are the three genuinely
 * optional fields and are omitted, not null-valued, when absent.
 */
@Value
@Builder(toBuilder = true)
@Jacksonized
@JsonPropertyOrder({
        "path", "env", "env_file", "secrets_file", "docs", "ty", "values",
        "constraint", "text_constraint", "text_form",
        "aliases", "env_aliases", "env_file_aliases", "secrets_file_aliases",
        "unreachable", "default", "default_value", "note", "required", "secret", "reserved"
})
public class Key {

    /** The document path, e.g. {@code csp.cloudflare.turnstile}. Unique across {@code keys}. */
    @NonNull
    String path;

    /** The variable supplying it directly. Null means no variable names it; see {@link #unreachable}. */
    @JsonInclude(JsonInclude.Include.ALWAYS)
    String env;

    /** The variable naming a file that holds it. */
    @JsonInclude(JsonInclude.Include.ALWAYS)
    @JsonProperty("env_file")
    String envFile;

    /** The file name inside the secrets directory that supplies it. */
    @JsonInclude(JsonInclude.Include.ALWAYS)
    @JsonProperty("secrets_file")
    String secretsFile;

    /** Prose describing the key. Empty when there was none. Markdown; the first paragraph is the summary. */
    @NonNull
    @Builder.Default
    String docs = "";

    /**
     * The producer's own name for the field's type, with any optionality stripped. Token text in
     * the producer's language, not a portable type name: read {@link #constraint} to check a
     * value, never this.
     */
    @JsonInclude(JsonInclude.Include.ALWAYS)
    String ty;

    /** The fixed set of values the key accepts. Empty when the key is not a choice. */
    @NonNull
    @Builder.Default
    List<String> values = List.of();

    /**
     * What the value must be once it is in the document, as JSON Schema keywords. Absent means
     * no check is possible; a consumer must not invent one.
     */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    Map<String, Object> constraint;

    /**
     * What the characters of an environment variable must be, before anything parses them.
     * Measured against {@code producer.loader}.
     */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    @JsonProperty("text_constraint")
    Map<String, Object> textConstraint;

    @NonNull
    @JsonProperty("text_form")
    TextForm textForm;

    /** Other document spellings of this key. A file using one loads, so a consumer must accept them. */
    @NonNull
    @Builder.Default
    List<String> aliases = List.of();

    /** The environment spellings of {@link #aliases}, derived by the producer. */
    @NonNull
    @Builder.Default
    @JsonProperty("env_aliases")
    List<String> envAliases = List.of();

    @NonNull
    @Builder.Default
    @JsonProperty("env_file_aliases")
    List<String> envFileAliases = List.of();

    @NonNull
    @Builder.Default
    @JsonProperty("secrets_file_aliases")
    List<String> secretsFileAliases = List.of();

    /** Why no environment variable is published, required exactly when {@link #env} is null. */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    UnreachableReason unreachable;

    /** The observed default, rendered for display. Null when the key has none. {@code <redacted>} for a secret. */
    @JsonProperty("default")
    @JsonInclude(JsonInclude.Include.ALWAYS)
    String defaultText;

    /**
     * The same default as {@link #defaultText}, as a value rather than as text. Null when the
     * key has none, indistinguishable from a default that is itself null — use
     * {@link #defaultText} and {@link #required} to tell those apart.
     */
    @JsonInclude(JsonInclude.Include.ALWAYS)
    @JsonProperty("default_value")
    Object defaultValue;

    /** Prose qualifying the default, e.g. {@code permanent} for a zero that means no expiry. */
    @JsonInclude(JsonInclude.Include.ALWAYS)
    String note;

    /** Whether some layer must supply the key. Not JSON Schema's {@code required}. */
    boolean required;

    /** Whether the value is a credential. A secret never carries a default, anywhere in the document. */
    boolean secret;

    /** Whether the loader reads it directly from the environment, before the layers exist. */
    boolean reserved;
}
