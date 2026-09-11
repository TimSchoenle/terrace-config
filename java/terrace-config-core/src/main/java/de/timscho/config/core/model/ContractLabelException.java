package de.timscho.config.core.model;

import java.util.List;

import lombok.Getter;

/**
 * Thrown by {@link Contract#verifyLabels} naming every label a built image gets wrong, not only
 * the first — a build that names one missing label and hides two is a second round trip through
 * a pipeline that already took minutes.
 */
@Getter
public final class ContractLabelException extends RuntimeException {

    /** Every label fault this contract found, in declaration order. */
    private final transient List<LabelFault> faults;

    public ContractLabelException(List<LabelFault> faults) {
        super(message(faults));
        this.faults = List.copyOf(faults);
    }

    private static String message(List<LabelFault> faults) {
        StringBuilder message = new StringBuilder();
        for (int i = 0; i < faults.size(); i++) {
            if (i > 0) {
                message.append('\n');
            }
            message.append(faults.get(i));
        }
        return message.toString();
    }
}
