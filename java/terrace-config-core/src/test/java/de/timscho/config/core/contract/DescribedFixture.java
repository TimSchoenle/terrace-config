package de.timscho.config.core.contract;

/** A package-private configuration type for {@link ConfigurationTypeTest}, with one field default. */
final class DescribedFixture {

    private final String value = "default";

    private DescribedFixture() {}

    String value() {
        return this.value;
    }
}
