package de.timscho.config.spring;

import de.timscho.config.annotations.Nested;
import de.timscho.config.annotations.Secret;
import de.timscho.config.annotations.TerraceConfig;

/**
 * An annotated fixture for {@link SpringContractProducerTest}, processed at compile time by
 * {@code terrace-config-processor} into a real {@code ServiceConfigDescriptor}.
 */
@TerraceConfig
class ServiceConfig {

    @Nested
    Github github;

    int port;

    @TerraceConfig
    static class Github {
        @Secret
        String token;
    }
}
