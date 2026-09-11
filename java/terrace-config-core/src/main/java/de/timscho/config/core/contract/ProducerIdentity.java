package de.timscho.config.core.contract;

import lombok.experimental.UtilityClass;

import de.timscho.config.core.model.Producer;

/**
 * What this module writes into every {@link Producer} it builds — the Java equivalent of the
 * Rust crate's {@code Producer::current}.
 *
 * <p>Not settable by a caller, and deliberately: a producer that could name itself something
 * else could publish a document whose reading rules are another implementation's, which is the
 * single claim in a contract's envelope a consumer cannot check for itself.
 */
@UtilityClass
public class ProducerIdentity {

    /** The implementation name this module writes as {@link Producer#getName()}. */
    public static final String NAME = "terrace-config-java";

    /**
     * This module's own version, read from the package's implementation version (set from the
     * build's own version at packaging time). Falls back to {@code 0.0.0-dev} when run from
     * classes rather than a packaged jar, e.g. under a test runner, where no manifest exists to
     * read it from.
     */
    public static String version() {
        String implementationVersion = ProducerIdentity.class.getPackage().getImplementationVersion();
        return implementationVersion != null ? implementationVersion : "0.0.0-dev";
    }

    /**
     * A {@link Producer} identifying this module, naming {@code loader} as the library whose
     * environment reads a document's {@code text_constraint}s were measured against.
     */
    public static Producer forLoader(String loader) {
        return Producer.builder().name(NAME).version(version()).loader(loader).build();
    }
}
