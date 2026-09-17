package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import java.util.List;
import lombok.Builder;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;

/**
 * The surface outside the loader's namespace — {@code spec/v1/contract.schema.json}'s
 * {@code #/$defs/external}. A positive declaration rather than a suppression list: a declared
 * variable is checked exactly like a configuration key, an ignored one may be misspelt freely.
 */
@Value
@Builder(toBuilder = true)
@Jacksonized
@JsonPropertyOrder({"env", "ignore", "unknown"})
public class External {

    @Builder.Default
    List<ExternalVar> env = List.of();

    /** Names nobody owns. Only a trailing {@code *} is a wildcard. */
    @Builder.Default
    List<String> ignore = List.of();

    ExternalUnknownPolicy unknown;
}
