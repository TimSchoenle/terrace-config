package de.timscho.config.spring;

import static org.assertj.core.api.Assertions.assertThat;

import org.junit.jupiter.api.Test;
import org.springframework.boot.autoconfigure.AutoConfigurations;
import org.springframework.boot.test.context.runner.ApplicationContextRunner;

import de.timscho.config.core.model.Contract;

/**
 * {@link TerraceContractAutoConfiguration} through Spring's own {@link ApplicationContextRunner},
 * which is what actually applies {@code @ConditionalOnProperty} and property binding rather than
 * calling the {@code @Bean} method directly.
 */
class TerraceContractAutoConfigurationTest {

    private final ApplicationContextRunner contextRunner = new ApplicationContextRunner()
            .withConfiguration(AutoConfigurations.of(TerraceContractAutoConfiguration.class));

    @Test
    void publishesNoContractBeanWithNoTypeConfigured() {
        contextRunner.run(context -> assertThat(context).doesNotHaveBean(Contract.class));
    }

    @Test
    void publishesAContractBeanWhenTypeAndPrefixAreConfigured() {
        contextRunner
                .withPropertyValues(
                        "terrace.contract.type=de.timscho.config.spring.ServiceConfig",
                        "terrace.contract.env-prefix=PORTFOLIO_",
                        "spring.application.name=portfolio")
                .run(context -> {
                    assertThat(context).hasSingleBean(Contract.class);
                    Contract contract = context.getBean(Contract.class);
                    assertThat(contract.getSchema().getDialect().getPrefix()).isEqualTo("PORTFOLIO_");
                    assertThat(contract.getApp().getName()).isEqualTo("portfolio");
                });
    }

    @Test
    void anAppNamePropertyOverridesTheSpringApplicationName() {
        contextRunner
                .withPropertyValues(
                        "terrace.contract.type=de.timscho.config.spring.ServiceConfig",
                        "terrace.contract.env-prefix=PORTFOLIO_",
                        "terrace.contract.app-name=explicit-name",
                        "spring.application.name=portfolio")
                .run(context -> assertThat(
                                context.getBean(Contract.class).getApp().getName())
                        .isEqualTo("explicit-name"));
    }

    @Test
    void failsClearlyWhenTheTypeHasNoGeneratedDescriptor() {
        contextRunner
                .withPropertyValues(
                        "terrace.contract.type=de.timscho.config.spring.NoSuchType",
                        "terrace.contract.env-prefix=PORTFOLIO_")
                .run(context -> assertThat(context).hasFailed());
    }

    @Test
    void failsClearlyWhenNoPrefixIsConfigured() {
        contextRunner
                .withPropertyValues("terrace.contract.type=de.timscho.config.spring.ServiceConfig")
                .run(context -> assertThat(context).hasFailed());
    }
}
