package de.timscho.config.tck.fixtures;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import de.timscho.config.core.descriptor.TypeDescriptor;
import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.spring.SpringContractProducer;
import java.util.Map;

/**
 * The same fixture types {@link FixtureContracts} assembles through the loader, produced through
 * {@code terrace-config-spring-boot}'s {@link SpringContractProducer} instead: Spring's dialect,
 * {@code producer.loader} {@code spring-boot}, and defaults read off a default-constructed
 * instance exactly as {@code SpringContractMain} reads them. Refinements and an external surface
 * are the loader cases' own; a Spring service declares neither through this producer.
 */
public final class SpringFixtureContracts {

    private static final ObjectMapper MAPPER = new ObjectMapper();

    private SpringFixtureContracts() {}

    /** {@link MinimalConfig} under {@code MINIMAL_}. */
    public static Contract minimal() {
        return produce(MinimalConfigDescriptor.DESCRIPTOR, "MINIMAL_", "minimal", new MinimalConfig());
    }

    /** {@link FullSurfaceConfig} under {@code PORTFOLIO_}. */
    public static Contract fullSurface() {
        return produce(FullSurfaceConfigDescriptor.DESCRIPTOR, "PORTFOLIO_", "portfolio", new FullSurfaceConfig());
    }

    /** {@link UnnameableConfig} under {@code EDGE_}. */
    public static Contract unnameableKey() {
        return produce(UnnameableConfigDescriptor.DESCRIPTOR, "EDGE_", "edge", new UnnameableConfig());
    }

    /** {@link RequiredEntriesConfig} under {@code SITE_}, without the library's refinements. */
    public static Contract requiredEntries() {
        return produce(RequiredEntriesConfigDescriptor.DESCRIPTOR, "SITE_", "site", new RequiredEntriesConfig());
    }

    /** {@link HandbookConfig} under {@code BOOK_}, without the handbook's refinements. */
    public static Contract elementPatterns() {
        return produce(HandbookConfigDescriptor.DESCRIPTOR, "BOOK_", "handbook", new HandbookConfig());
    }

    private static Contract produce(
            final TypeDescriptor descriptor, final String prefix, final String name, final Object defaults) {
        final App app = App.builder().name(name).version("1.0.0").build();
        return SpringContractProducer.produce(
                descriptor, prefix, app, MAPPER.convertValue(defaults, new TypeReference<Map<String, Object>>() {}));
    }
}
