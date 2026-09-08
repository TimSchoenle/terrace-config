package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * How to read the characters an environment variable holds, before checking the result against
 * {@code constraint}. From {@code spec/v1/contract.schema.json}'s {@code text_form}.
 *
 * <p>Always present on a key, so that a null {@code text_constraint} has one meaning:
 * {@link #TEXT} says any text is fine, {@link #UNKNOWN} says nothing could be determined. The
 * read each form names belongs to {@code producer.loader}, not to the document.
 */
public enum TextForm {

    @JsonProperty("text")
    TEXT,

    @JsonProperty("integer")
    INTEGER,

    @JsonProperty("boolean")
    BOOLEAN,

    @JsonProperty("choice")
    CHOICE,

    @JsonProperty("structured")
    STRUCTURED,

    @JsonProperty("unknown")
    UNKNOWN
}
