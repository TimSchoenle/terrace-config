package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import lombok.Builder;
import lombok.NonNull;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;

/**
 * Which implementation wrote the document — {@code spec/v1/contract.schema.json}'s
 * {@code #/$defs/producer}.
 *
 * <p>Not a fact about the configuration. {@link #loader} is what makes every
 * {@code text_constraint} in the document safe to apply: those patterns were measured against a
 * named library's environment reads, and another library has different answers. A consumer that
 * does not recognise {@link #loader} must not apply the reads in {@code spec/v1/FORMAT.md}.
 */
@Value
@Builder
@Jacksonized
@JsonPropertyOrder({"name", "version", "loader"})
public class Producer {

    /** The implementation, e.g. {@code terrace-config-java}. A stable identifier, not a display name. */
    @NonNull
    String name;

    /** The implementation's own version. Not {@code app.version}, which versions the described service. */
    @NonNull
    String version;

    /**
     * The library whose environment reads the text constraints were measured against, e.g.
     * {@code terrace-java}. Named rather than restated: a vocabulary describing every way a
     * binder might read a string is a language two implementations can disagree in.
     */
    @NonNull
    String loader;
}
