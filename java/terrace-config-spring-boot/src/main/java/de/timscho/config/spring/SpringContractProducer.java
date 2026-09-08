package de.timscho.config.spring;

import java.util.Set;

import de.timscho.config.core.contract.ContractAssembler;
import de.timscho.config.core.contract.ProducerIdentity;
import de.timscho.config.core.descriptor.SchemaAssembler;
import de.timscho.config.core.descriptor.TypeDescriptor;
import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.External;
import de.timscho.config.core.model.Producer;
import de.timscho.config.core.model.Schema;

/**
 * Assembles a full {@link Contract} for a Spring service — the tier-1 producer this module ships,
 * combining {@link SpringDialect}, {@link SchemaAssembler} and {@link ContractAssembler} the way
 * {@code terrace-config-loader}'s {@code TerraceLoader.schemaAt} combines its own {@code Dialect}
 * with the same two.
 *
 * <p><b>Not an auto-configuration.</b> Unlike {@link FileIndirectionEnvironmentPostProcessor},
 * which runs unconditionally because every Spring service needs {@code _FILE} indirection the
 * same way, a service's own {@code @TerraceConfig} type and environment prefix are things only
 * that service knows — there is no annotation on the classpath a starter could scan for that
 * would not also have to invent a place to declare the prefix and the {@link App} metadata. A
 * consuming service calls {@link #produce} from its own {@code @Configuration} class, once, and
 * publishes the result however it publishes any other {@link Contract} (see {@code
 * Contract#toDockerfileBlock}).
 *
 * <p><b>Schema.loader is empty.</b> {@code terrace-config-loader}'s own three loader-read
 * variables ({@code config}, {@code secrets_dir}, each {@code reserve}d key) have no equivalent
 * here: Spring's {@code Binder} decides what the layers are from its own property sources, not
 * from a variable this module reads first, so there is nothing to publish under {@code
 * schema.loader} — an empty list is the honest answer, not an omission.
 */
public final class SpringContractProducer {

    private SpringContractProducer() {
    }

    /** {@link #produce(TypeDescriptor, String, SpringDialect, App, External)} using {@link SpringDialect#standard()}
     * and no declared external surface. */
    public static Contract produce(TypeDescriptor descriptor, String prefix, App app) {
        return produce(descriptor, prefix, SpringDialect.standard(), app, ContractAssembler.noExternalSurface());
    }

    /**
     * Assembles and validates the {@link Contract} for a service whose configuration type is
     * described by {@code descriptor}, reachable under {@code prefix} through {@code
     * springDialect}'s nesting and {@code _FILE} indirection conventions.
     *
     * @throws de.timscho.config.core.refusal.ContractRefusalException the first of {@code
     *                                                                  spec/v1/FORMAT.md}'s eight
     *                                                                  refusals this contract
     *                                                                  would violate
     */
    public static Contract produce(TypeDescriptor descriptor, String prefix, SpringDialect springDialect,
                                    App app, External external) {
        Dialect dialect = Dialect.builder()
                .prefix(prefix)
                .nestingSeparator(springDialect.separator())
                .indirectionSuffix(springDialect.indirectionSuffix())
                .build();

        Schema schema = SchemaAssembler.assemble(descriptor, dialect, Set.of());
        Producer producer = ProducerIdentity.forLoader("spring-boot");
        return ContractAssembler.assemble(schema, app, producer, external);
    }
}
