package de.timscho.config.spring;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import de.timscho.config.core.io.ContractCodec;
import de.timscho.config.core.model.Contract;
import java.util.Map;

/**
 * The Jackson half of {@link SpringContractMain}, in a class of its own so that the main class
 * links without Jackson on the classpath and can say which module is missing instead of failing
 * with a {@link NoClassDefFoundError} while it is verified.
 */
final class GeneratorCodec {

    private GeneratorCodec() {}

    /**
     * {@code instance}'s own field values, nested the way {@link
     * de.timscho.config.core.model.Schema#withDefaultsFromValue} reads them. Not the mapper
     * Spring's binder binds a real configuration with: this one only converts an instance this
     * process just constructed, never untrusted input.
     */
    static Map<String, Object> defaultsOf(final Object instance) {
        return new ObjectMapper().convertValue(instance, new TypeReference<Map<String, Object>>() {});
    }

    static byte[] write(final Contract contract) {
        return ContractCodec.write(contract);
    }
}
