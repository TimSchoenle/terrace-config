package de.timscho.config.core.model;

import java.util.List;
import java.util.Map;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import lombok.Builder;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;
import org.jspecify.annotations.Nullable;

/**
 * A variable this image reads that is nobody's configuration key — {@code
 * spec/v1/contract.schema.json}'s {@code #/$defs/external_var}. Never carries {@code
 * dialect.prefix}: everything in that namespace is a configuration key (see
 * {@link de.timscho.config.core.refusal.ExternalVariableInPrefixException}).
 */
@Value
@Builder(toBuilder = true)
@Jacksonized
@JsonPropertyOrder({
    "name",
    "owner",
    "docs",
    "ty",
    "values",
    "constraint",
    "text_constraint",
    "text_form",
    "default",
    "required",
    "secret"
})
public class ExternalVar {

    String name;

    /** What reads it — a toolchain, a base image, a library. */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    @Nullable String owner;

    @Builder.Default
    String docs = "";

    @JsonInclude(JsonInclude.Include.NON_NULL)
    @Nullable String ty;

    @Builder.Default
    List<String> values = List.of();

    @JsonInclude(JsonInclude.Include.NON_NULL)
    @Nullable Map<String, Object> constraint;

    @JsonInclude(JsonInclude.Include.NON_NULL)
    @JsonProperty("text_constraint")
    @Nullable Map<String, Object> textConstraint;

    @JsonProperty("text_form")
    TextForm textForm;

    @JsonProperty("default")
    @JsonInclude(JsonInclude.Include.NON_NULL)
    @Nullable String defaultText;

    boolean required;

    /** A secret never carries a default (see {@link de.timscho.config.core.refusal.SecretWithDefaultException}). */
    boolean secret;
}
