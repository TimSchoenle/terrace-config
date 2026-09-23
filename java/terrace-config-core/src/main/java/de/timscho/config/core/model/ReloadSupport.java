package de.timscho.config.core.model;

import com.fasterxml.jackson.annotation.JsonIgnore;
import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import java.util.List;
import lombok.Builder;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;

/**
 * Whether an image applies a configuration change without restarting, and through which layers —
 * {@code spec/v1/contract.schema.json}'s {@code #/$defs/reload}, published as {@code schema.reload}.
 *
 * <p>A fact about the binary rather than about {@code producer.loader}. Absent means undeclared,
 * which a consumer treats as {@link ReloadMode#NONE}.
 */
@Value
@Builder(toBuilder = true)
@Jacksonized
@JsonPropertyOrder({"mode", "layers"})
public class ReloadSupport {

    /** How a change is applied. */
    ReloadMode mode;

    /** The layers watched for one. Empty exactly when {@link #mode} is {@link ReloadMode#NONE}. */
    @Builder.Default
    List<ReloadLayer> layers = List.of();

    /** The image applies nothing after start. */
    public static ReloadSupport none() {
        return ReloadSupport.builder().mode(ReloadMode.NONE).build();
    }

    /** The image rebuilds on a change to any of the three file layers. */
    public static ReloadSupport rebuild() {
        return ReloadSupport.builder()
                .mode(ReloadMode.REBUILD)
                .layers(List.of(ReloadLayer.DOCUMENT, ReloadLayer.SECRETS_DIR, ReloadLayer.ENV_FILE))
                .build();
    }

    /** Whether a change can be applied after start at all: a rebuild over at least one layer. */
    @JsonIgnore
    public boolean rebuilds() {
        return this.mode == ReloadMode.REBUILD && !this.layers.isEmpty();
    }
}
