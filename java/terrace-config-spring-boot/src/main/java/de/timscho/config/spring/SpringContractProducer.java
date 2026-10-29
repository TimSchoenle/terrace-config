package de.timscho.config.spring;

import de.timscho.config.core.contract.ContractAssembler;
import de.timscho.config.core.contract.ProducerIdentity;
import de.timscho.config.core.descriptor.SchemaAssembler;
import de.timscho.config.core.descriptor.TypeDescriptor;
import de.timscho.config.core.model.App;
import de.timscho.config.core.model.Contract;
import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.External;
import de.timscho.config.core.model.Key;
import de.timscho.config.core.model.Producer;
import de.timscho.config.core.model.Schema;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * Assembles a full {@link Contract} for a Spring service: the tier-1 producer this module ships,
 * combining {@link SpringDialect}, {@link SchemaAssembler} and {@link ContractAssembler} the way
 * {@code terrace-config-loader}'s {@code TerraceLoader.schemaAt} combines its own {@code Dialect}
 * with the same two.
 *
 * <p><b>Built at build time, not at boot.</b> A contract is a build artefact: it describes the
 * image, ships inside it, and is conformed before the image exists. {@link SpringContractMain}
 * calls {@link #produce} from the service's build, with the defaults read off a
 * default-constructed instance of the properties class, and the Gradle plugin's {@code
 * terraceContract} task runs it. A contract assembled inside a running application would need the
 * application to boot first, and would be a second, unconformed copy of the same document.
 *
 * <p><b>Schema.loader is empty.</b> {@code terrace-config-loader}'s own three loader-read variables
 * ({@code config}, {@code secrets_dir}, each {@code reserve}d key) have no equivalent here:
 * Spring's {@code Binder} decides what the layers are from its own property sources, not from a
 * variable this module reads first, so there is nothing to publish under {@code schema.loader}, and
 * an empty list is the honest answer, not an omission.
 */
public final class SpringContractProducer {

    private SpringContractProducer() {}

    /**
     * {@return {@link #produce(TypeDescriptor, String, SpringDialect, App, External)} using {@link
     * SpringDialect#standard()} and no declared external surface}
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
     * @return the validated contract
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

    /**
     * {@return {@link #produce(TypeDescriptor, String, SpringDialect, App, External, Map)} using
     * {@link SpringDialect#standard()} and no declared external surface}
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
     * {@return {@link #produce(TypeDescriptor, String, SpringDialect, App, External)}, additionally
     * filling in each non-required key's observed default from {@code defaults}, an already-nested
     * map, typically {@code objectMapper.convertValue(defaultInstance, Map.class)} run against a
     * default-constructed instance of the type {@code descriptor} describes} See {@link
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

        final Schema schema = withoutTextConstraints(
                SchemaAssembler.assemble(descriptor, dialect, Set.of()).withDefaultsFromValue(defaults));
        final Producer producer = ProducerIdentity.forLoader("spring-boot");
        return ContractAssembler.assemble(schema, app, producer, external);
    }

    /**
     * {@code schema} with no key's {@code text_constraint}. The shared patterns describe the
     * strict reads of {@code FORMAT.md}, and Spring's {@code Binder} accepts more: {@code on} and
     * {@code yes} as booleans, hexadecimal integers, enum constants in any case, comma-separated
     * lists. Nothing has measured those reads, and {@code spec/v1/PRODUCER.md} says to omit the
     * pattern rather than publish one stricter than the loader.
     */
    private static Schema withoutTextConstraints(final Schema schema) {
        final List<Key> keys = new ArrayList<>(schema.getKeys().size());
        for (final Key key : schema.getKeys()) {
            keys.add(key.toBuilder().textConstraint(null).build());
        }
        return schema.toBuilder().keys(List.copyOf(keys)).build();
    }
}
