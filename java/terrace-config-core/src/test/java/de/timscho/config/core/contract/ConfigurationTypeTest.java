package de.timscho.config.core.contract;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import org.junit.jupiter.api.Test;

/**
 * A configuration type named on a command line. {@link DescribedFixtureDescriptor} is a
 * hand-written stand-in shaped like the processor's output: this module cannot run the processor
 * on its own tests, and the lookup only reads the class name and the field. A nested type's
 * lookup is covered with a real generated descriptor in {@code terrace-config-loader}'s {@code
 * ContractMainTest}.
 */
class ConfigurationTypeTest {

    static final class NotGenerated {}

    private static final ClassLoader LOADER = ConfigurationTypeTest.class.getClassLoader();

    @Test
    void findsTheDescriptorBesideTheTypeByItsBinaryName() {
        ConfigurationType type = ConfigurationType.named(DescribedFixture.class.getName(), LOADER);

        assertThat(type.descriptor().qualifiedName()).isEqualTo("described");
    }

    @Test
    void constructsTheDefaultInstanceEvenWhenTheTypeIsNotPublic() {
        Object instance = ConfigurationType.named(DescribedFixture.class.getName(), LOADER)
                .defaultInstance();

        assertThat(instance).isInstanceOf(DescribedFixture.class);
        assertThat(((DescribedFixture) instance).value()).isEqualTo("default");
    }

    @Test
    void saysWhatIsMissingWhenTheTypeOrItsDescriptorIsNotThere() {
        assertThatThrownBy(() -> ConfigurationType.named("com.example.Missing", LOADER))
                .isInstanceOf(IllegalArgumentException.class)
                .hasMessage("com.example.Missing is not on the classpath");
        assertThatThrownBy(() -> new ConfigurationType(NotGenerated.class).descriptor())
                .isInstanceOf(IllegalArgumentException.class)
                .hasMessageContaining("NotGeneratedDescriptor was not found")
                .hasMessageContaining("@TerraceConfig");
    }

    @Test
    void aTypeWithoutANoArgumentConstructorHasNoDefaults() {
        assertThatThrownBy(() -> new ConfigurationType(Integer.class).defaultInstance())
                .isInstanceOf(IllegalArgumentException.class)
                .hasMessageContaining("needs a no-argument constructor");
    }
}
