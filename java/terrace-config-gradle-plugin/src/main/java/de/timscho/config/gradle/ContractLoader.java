package de.timscho.config.gradle;

import java.util.Locale;
import org.jspecify.annotations.Nullable;

/**
 * The Java producers a service can emit its contract through, each named by the {@code
 * producer.loader} it writes.
 *
 * <p>Each carries the build-time entry point the emission runs, the tier the producer claims in
 * {@code java/README.md}, and the module whose presence on the runtime classpath identifies it.
 */
enum ContractLoader {

    /** {@code terrace-config-loader}, the vanilla five-layer loader. */
    TERRACE_JAVA("terrace-java", "de.timscho.config.loader.ContractMain", 2, "terrace-config-loader"),

    /** {@code terrace-config-spring-boot}, binding through Spring's own {@code Binder}. */
    SPRING_BOOT("spring-boot", "de.timscho.config.spring.SpringContractMain", 1, "terrace-config-spring-boot");

    private final String id;
    private final String mainClass;
    private final int tier;
    private final String module;

    ContractLoader(final String id, final String mainClass, final int tier, final String module) {
        this.id = id;
        this.mainClass = mainClass;
        this.tier = tier;
        this.module = module;
    }

    /** The {@code producer.loader} value, which is also how the extension names it. */
    String id() {
        return this.id;
    }

    /** The build-time entry point that emits this producer's contract. */
    String mainClass() {
        return this.mainClass;
    }

    /** The conformance tier this producer claims, and so the one its contract is held to. */
    int tier() {
        return this.tier;
    }

    /** The module name whose presence on the runtime classpath means this producer is in use. */
    String module() {
        return this.module;
    }

    /**
     * The producer with this {@code id}.
     *
     * @throws IllegalArgumentException for any other value, naming the accepted ones
     */
    static ContractLoader byId(final String id) {
        for (final ContractLoader loader : values()) {
            if (loader.id.equals(id.toLowerCase(Locale.ROOT))) {
                return loader;
            }
        }
        throw new IllegalArgumentException("terraceContract.loader is `" + id + "`; expected `" + TERRACE_JAVA.id
                + "` or `" + SPRING_BOOT.id + "`");
    }

    /** The producer whose {@link #module()} this is, or {@code null} when it is neither. */
    static @Nullable ContractLoader byModule(final String module) {
        for (final ContractLoader loader : values()) {
            if (loader.module.equals(module)) {
                return loader;
            }
        }
        return null;
    }
}
