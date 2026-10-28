package de.timscho.config.gradle;

import org.gradle.api.file.RegularFileProperty;
import org.gradle.api.provider.ListProperty;
import org.gradle.api.provider.Property;

/**
 * The {@code terraceContract { }} block: what the service's contract is emitted from, and the build
 * identity it is stamped with.
 *
 * <pre>{@code
 * terraceContract {
 *     type = "com.example.Config"   // the root @TerraceConfig type, by binary name
 *     prefix = "MYAPP_"
 *     reserve.add("MYAPP_PROFILE")  // terrace-java only
 * }
 * }</pre>
 *
 * <p>Only {@link #getType()} and {@link #getPrefix()} have no default. Each stamped value is
 * resolved, first one set wins, from this block, then the Gradle property {@code
 * terraceContract.<name>} ({@code -PterraceContract.revision=...}), then the environment variable
 * named beside it; {@link #getAppVersion()} falls back to the project's version, and {@link
 * #getSource()} is read from this block alone, since where the source lives does not change from
 * one build to the next.
 */
public abstract class TerraceContractExtension {

    /** Creates the extension. */
    public TerraceContractExtension() {
        // Gradle generates the subclass that implements the abstract properties.
    }

    /**
     * {@return the root {@code @TerraceConfig} type, by binary name ({@code
     * com.example.Outer$Inner})}
     */
    public abstract Property<String> getType();

    /** {@return the environment prefix, taken verbatim ({@code MYAPP_})} */
    public abstract Property<String> getPrefix();

    /** {@return the contract's {@code app.name}} Defaults to the project's name. */
    public abstract Property<String> getAppName();

    /**
     * {@return reserved keys in their full environment spelling ({@code MYAPP_PROFILE})} Only
     * {@code terrace-java} reads reserved keys; setting one for {@code spring-boot} fails the
     * emission.
     */
    public abstract ListProperty<String> getReserve();

    /**
     * {@return further options passed to the entry point as they are, for the producer settings
     * this block has no property for ({@code --nesting-separator}, {@code --file-suffix}, and
     * {@code terrace-java}'s {@code --config-var}, {@code --secrets-dir-var} and {@code
     * --default-config-path})} Pass the same values the service boots with.
     */
    public abstract ListProperty<String> getProducerArguments();

    /**
     * {@return which producer emits the contract: {@code terrace-java} or {@code spring-boot}}
     * Defaults to the one whose module ({@code terrace-config-loader} or {@code
     * terrace-config-spring-boot}) is on the runtime classpath, and must be set when both or
     * neither is.
     */
    public abstract Property<String> getLoader();

    /**
     * {@return the tier {@code terrace-contract conform} holds the stamped contract to} Defaults to
     * the producer's own claim: 2 for {@code terrace-java}, 1 for {@code spring-boot}.
     */
    public abstract Property<Integer> getTier();

    /**
     * {@return {@code app.version}: {@code terraceContract.appVersion}, then the project's version
     * unless it is {@code unspecified}}
     */
    public abstract Property<String> getAppVersion();

    /**
     * {@return {@code app.revision}: {@code terraceContract.revision}, then {@code
     * TERRACE_CONTRACT_REVISION}}
     */
    public abstract Property<String> getRevision();

    /**
     * {@return {@code app.created}, as RFC 3339: {@code terraceContract.created}, then {@code
     * TERRACE_CONTRACT_CREATED}} Left unset by default, so two builds of one commit stamp the same
     * document.
     */
    public abstract Property<String> getCreated();

    /** {@return {@code app.source}, where the source lives} */
    public abstract Property<String> getSource();

    /**
     * {@return the {@code terrace-contract} binary} Defaults to {@code TERRACE_CONTRACT_BIN} (a
     * relative path is resolved against the root project), then {@code PATH}, and nothing else.
     */
    public abstract RegularFileProperty getExecutable();

    /**
     * {@return where {@code terraceContract} writes the stamped contract} Defaults to {@code
     * build/terrace/contract.json}.
     */
    public abstract RegularFileProperty getOutput();

    /**
     * {@return the committed copy {@code terraceContractCheck} compares the unstamped emission
     * with, and {@code terraceContractUpdate} rewrites} Defaults to {@code contract.json} in the
     * project directory.
     */
    public abstract RegularFileProperty getCommittedContract();
}
