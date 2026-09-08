package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import lombok.Builder;
import lombok.NonNull;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;

/**
 * Which build the contract describes — {@code spec/v1/contract.schema.json}'s
 * {@code #/$defs/app}.
 *
 * <p>Every field moves independently of the configuration surface, which is why they are
 * collected: a consumer diffing two contracts to see whether the configuration changed diffs
 * everything except this. Only {@link #name} is required; the rest are omitted, not
 * {@code null}-valued, when the build did not supply them.
 */
@Value
@Builder
@Jacksonized
@JsonPropertyOrder({"name", "version", "revision", "created", "source"})
@JsonInclude(JsonInclude.Include.NON_NULL)
public class App {

    @NonNull
    String name;

    String version;

    /** The commit the image was built from. */
    String revision;

    /** Build time, RFC 3339. */
    String created;

    /** Where the source lives. */
    String source;
}
