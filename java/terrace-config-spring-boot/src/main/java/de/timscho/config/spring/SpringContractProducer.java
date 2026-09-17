package de.timscho.config.spring;

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
import java.util.Map;
import java.util.Set;

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

    private SpringContractProducer() {}

    /** {@link #produce(TypeDescriptor, String, SpringDialect, App, External)} using {@link SpringDialect#standard()}
     * and no declared external surface.
     *
     * @param descriptor the configuration type's own field-level descriptor
     * @param prefix     the environment prefix keys are reachable under
     * @param app        the published contract's {@code app} metadata
     */
    public static Contract produce(final TypeDescriptor descriptor, final String prefix, final App app) {
        return produce(descriptor, prefix, SpringDialect.standard(), app, ContractAssembler.noExternalSurface());
    }

    /**
     * Assembles and validates the {@link Contract} for a service whose configuration type is
     * described by {@code descriptor}, reachable under {@code prefix} through {@code
     * springDialect}'s nesting and {@code _FILE} indirection conventions.
     *
     * @param descriptor    the configuration type's own field-level descriptor
     * @param prefix        the environment prefix keys are reachable under
     * @param springDialect the nesting-separator and indirection-suffix conventions to spell keys with
     * @param app           the published contract's {@code app} metadata
     * @param external      the service's declared external variable surface
     * @throws de.timscho.config.core.refusal.ContractRefusalException the first of {@code
     *                                                                  spec/v1/FORMAT.md}'s eight
     *                                                                  refusals this contract
     *                                                                  would violate
     */
    public static Contract produce(
            final TypeDescriptor descriptor,
            final String prefix,
            final SpringDialect springDialect,
            final App app,
            final External external) {
        return produce(descriptor, prefix, springDialect, app, external, Map.of());
    }

    /** {@link #produce(TypeDescriptor, String, SpringDialect, App, External, Map)} using {@link
     * SpringDialect#standard()} and no declared external surface.
     *
     * @param descriptor the configuration type's own field-level descriptor
     * @param prefix     the environment prefix keys are reachable under
     * @param app        the published contract's {@code app} metadata
     * @param defaults   each non-required key's observed default, already nested as a {@code Map}
     */
    public static Contract produce(
            final TypeDescriptor descriptor, final String prefix, final App app, final Map<String, Object> defaults) {
        return produce(
                descriptor, prefix, SpringDialect.standard(), app, ContractAssembler.noExternalSurface(), defaults);
    }

    /**
     * {@link #produce(TypeDescriptor, String, SpringDialect, App, External)}, additionally filling
     * in each non-required key's observed default from {@code defaults} — an already-nested map,
     * typically {@code objectMapper.convertValue(defaultInstance, Map.class)} run against a
     * default-constructed instance of the type {@code descriptor} describes. See {@link
     * Schema#withDefaultsFromValue} for exactly what filling in means; {@code Map.of()} (what the
     * four-argument overload above passes) leaves every key's default unset, the same as never
     * calling this method at all.
     *
     * @param descriptor    the configuration type's own field-level descriptor
     * @param prefix        the environment prefix keys are reachable under
     * @param springDialect the nesting-separator and indirection-suffix conventions to spell keys with
     * @param app           the published contract's {@code app} metadata
     * @param external      the service's declared external variable surface
     * @param defaults      each non-required key's observed default, already nested as a {@code Map}
     * @throws de.timscho.config.core.refusal.ContractRefusalException the first of {@code
     *                                                                  spec/v1/FORMAT.md}'s eight
     *                                                                  refusals this contract
     *                                                                  would violate
     */
    public static Contract produce(
            final TypeDescriptor descriptor,
            final String prefix,
            final SpringDialect springDialect,
            final App app,
            final External external,
            final Map<String, Object> defaults) {
        final Dialect dialect = Dialect.builder()
                .prefix(prefix)
                .nestingSeparator(springDialect.separator())
                .indirectionSuffix(springDialect.indirectionSuffix())
                .build();

        final Schema schema =
                SchemaAssembler.assemble(descriptor, dialect, Set.of()).withDefaultsFromValue(defaults);
        final Producer producer = ProducerIdentity.forLoader("spring-boot");
        return ContractAssembler.assemble(schema, app, producer, external);
    }
}
