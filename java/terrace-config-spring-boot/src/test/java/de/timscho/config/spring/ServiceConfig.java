package de.timscho.config.spring;

import de.timscho.config.annotations.Nested;
import de.timscho.config.annotations.Secret;
import de.timscho.config.annotations.TerraceConfig;
import lombok.Getter;

/**
 * An annotated fixture for {@link SpringContractProducerTest}, processed at compile time by
 * {@code terrace-config-processor} into a real {@code ServiceConfigDescriptor}. Getters, so
 * {@link SpringContractMainTest} can read its defaults the way a real properties class's are read.
 */
@TerraceConfig
@Getter
class ServiceConfig {

    @Nested
    Github github;

    int port;

    @TerraceConfig
    @Getter
    static class Github {
        @Secret
        String token;
    }
}
