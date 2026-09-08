package de.timscho.config.spring;

import org.junit.jupiter.api.Test;

import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.refusal.EmptyPrefixException;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

/**
 * {@link SpringContractProducer#produce} against a real generated descriptor
 * ({@link ServiceConfig}/{@code ServiceConfigDescriptor}), proving the whole path from an
 * annotated type to a validated {@link Contract} — {@code SpringDialect}'s single-underscore
 * nesting, an empty {@code schema.loader}, and the eight refusals still firing through
 * {@code ContractValidator}.
 */
class SpringContractProducerTest {

    private static final App APP = App.builder().name("portfolio").version("v2.5.0").build();

    @Test
    void producesAValidatedContractFromAnAnnotatedType() {
        Contract contract = SpringContractProducer.produce(ServiceConfigDescriptor.DESCRIPTOR, "PORTFOLIO_", APP);

        assertThat(contract.getSchema().getDialect().getPrefix()).isEqualTo("PORTFOLIO_");
        assertThat(contract.getSchema().getDialect().getNestingSeparator()).isEqualTo("_");
        assertThat(contract.getSchema().getLoader()).isEmpty();
        assertThat(contract.getProducer().getLoader()).isEqualTo("spring-boot");

        assertThat(contract.getSchema().getKeys())
                .extracting(Key::getPath)
                .containsExactlyInAnyOrder("github.token", "port");

        Key token = contract.getSchema().getKeys().stream()
                .filter(key -> key.getPath().equals("github.token"))
                .findFirst()
                .orElseThrow();
        assertThat(token.isSecret()).isTrue();
        assertThat(token.getEnv()).isEqualTo("PORTFOLIO_GITHUB_TOKEN");

        assertThat(contract.getJsonSchema().schemaDialect())
                .isEqualTo("http://json-schema.org/draft-07/schema#");
    }

    @Test
    void refusesAnEmptyPrefix() {
        assertThatThrownBy(() -> SpringContractProducer.produce(ServiceConfigDescriptor.DESCRIPTOR, "", APP))
                .isInstanceOf(EmptyPrefixException.class);
    }
}
