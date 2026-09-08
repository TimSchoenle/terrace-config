package de.timscho.config.core.model;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

import com.fasterxml.jackson.annotation.JsonIgnore;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.annotation.JsonPropertyOrder;
import lombok.Builder;
import lombok.NonNull;
import lombok.Value;
import lombok.extern.jackson.Jacksonized;

/**
 * The whole document a producer publishes — {@code spec/v1/contract.schema.json}'s root object.
 *
 * <p>This is the model {@code -loader} and {@code -spring-boot} build and {@code -spec-tck}
 * compares against the corpus; it knows nothing about either binder (see {@code
 * java/README.md}'s "Separation of concerns").
 */
@Value
@Builder(toBuilder = true)
@Jacksonized
@JsonPropertyOrder({"terrace_contract", "producer", "app", "schema", "json_schema", "external"})
public class Contract {

    /** The current envelope version. Every {@link Contract} built by this module carries this value. */
    public static final int ENVELOPE_VERSION = 1;

    /** The OCI artifact type a published contract is attached to its image under. */
    public static final String ARTIFACT_TYPE = "application/vnd.terrace.config-schema.v1+json";

    /** The image label carrying {@link #ENVELOPE_VERSION}. */
    public static final String LABEL_VERSION = "dev.terrace.config.contract.version";

    /** The image label naming where the contract is embedded in the image's own filesystem. */
    public static final String LABEL_PATH = "dev.terrace.config.contract.path";

    /** The image label carrying the loader's environment prefix, e.g. {@code PORTFOLIO_}. */
    public static final String LABEL_PREFIX = "dev.terrace.config.prefix";

    /** Where {@link #LABEL_PATH} points unless a build says otherwise. */
    public static final String DEFAULT_PATH = "/config/contract.json";

    /** Opens the region of a Dockerfile {@link #toDockerfileBlock} owns. */
    public static final String MARKER_BEGIN = "# terrace-config:labels:begin";

    /** Closes the region {@link #MARKER_BEGIN} opens. */
    public static final String MARKER_END = "# terrace-config:labels:end";

    /**
     * The version of this envelope's shape. A consumer refuses a version it was not written
     * against rather than reading it optimistically.
     */
    @JsonProperty("terrace_contract")
    @Builder.Default
    int terraceContract = ENVELOPE_VERSION;

    @NonNull
    Producer producer;

    @NonNull
    App app;

    @NonNull
    Schema schema;

    @NonNull
    @JsonProperty("json_schema")
    JsonSchemaDocument jsonSchema;

    @NonNull
    External external;

    /**
     * The image labels that make this contract discoverable, given where it was embedded. All
     * three are constants for a given service, so a build needs no argument to interpolate and
     * no host-side generator run to feed {@code --label}.
     */
    @JsonIgnore
    public List<Map.Entry<String, String>> labels(String path) {
        List<Map.Entry<String, String>> labels = new ArrayList<>();
        labels.add(Map.entry(LABEL_VERSION, Integer.toString(terraceContract)));
        labels.add(Map.entry(LABEL_PATH, path));
        labels.add(Map.entry(LABEL_PREFIX, schema.getDialect().getPrefix()));
        return labels;
    }

    /**
     * {@link #labels} as a {@code LABEL} instruction, ready to paste into a Dockerfile. Ends with
     * a newline; no trailing backslash, so a following instruction needs no separator.
     */
    @JsonIgnore
    public String toDockerfileLabels(String path) {
        StringBuilder rendered = new StringBuilder("LABEL ");
        List<Map.Entry<String, String>> labels = labels(path);
        for (int index = 0; index < labels.size(); index++) {
            if (index > 0) {
                rendered.append(" \\\n      ");
            }
            Map.Entry<String, String> label = labels.get(index);
            rendered.append(label.getKey()).append("=\"");
            for (int i = 0; i < label.getValue().length(); i++) {
                char character = label.getValue().charAt(i);
                if (character == '"' || character == '\\') {
                    rendered.append('\\');
                }
                rendered.append(character);
            }
            rendered.append('"');
        }
        rendered.append('\n');
        return rendered.toString();
    }

    /** {@link #toDockerfileLabels} wrapped in {@link #MARKER_BEGIN} and {@link #MARKER_END}. */
    @JsonIgnore
    public String toDockerfileBlock(String path) {
        return MARKER_BEGIN + "\n" + toDockerfileLabels(path) + MARKER_END + "\n";
    }

    /**
     * Every label of this contract a built image gets wrong, in declaration order. Empty means
     * the image carries them all; extra labels are ignored.
     */
    @JsonIgnore
    public List<LabelFault> checkLabels(String path, Map<String, String> imageLabels) {
        List<LabelFault> faults = new ArrayList<>();
        for (Map.Entry<String, String> expected : labels(path)) {
            String found = imageLabels.get(expected.getKey());
            if (found == null) {
                faults.add(LabelFault.missing(expected.getKey()));
            } else if (!found.equals(expected.getValue())) {
                faults.add(LabelFault.mismatch(expected.getKey(), found, expected.getValue()));
            }
        }
        return faults;
    }

    /**
     * Check that a built image carries these labels.
     *
     * @throws ContractLabelException naming every label that is missing or wrong, all of them,
     *                                not the first
     */
    @JsonIgnore
    public void verifyLabels(String path, Map<String, String> imageLabels) {
        List<LabelFault> faults = checkLabels(path, imageLabels);
        if (!faults.isEmpty()) {
            throw new ContractLabelException(faults);
        }
    }
}
