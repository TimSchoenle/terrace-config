package de.timscho.config.core.contract;

import de.timscho.config.core.descriptor.TypeDescriptor;
import java.util.List;

/** What {@code terrace-config-processor} would generate beside {@link DescribedFixture}. */
public final class DescribedFixtureDescriptor {

    public static final TypeDescriptor DESCRIPTOR =
            new TypeDescriptor("described", TypeDescriptor.Kind.STRUCT, List.of(), List.of(), false);

    private DescribedFixtureDescriptor() {}
}
