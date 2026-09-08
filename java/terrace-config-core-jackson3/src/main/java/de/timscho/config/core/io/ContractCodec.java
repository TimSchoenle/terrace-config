package de.timscho.config.core.io;

import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

import tools.jackson.databind.DeserializationFeature;
import tools.jackson.databind.ObjectMapper;
import tools.jackson.databind.SerializationFeature;
import tools.jackson.databind.json.JsonMapper;

import de.timscho.config.core.model.Contract;

/**
 * Reads and writes a {@link Contract} as the {@code json} rendering — {@code
 * spec/v1/contract.schema.json}'s envelope, byte for byte.
 *
 * <p>Mirrors {@code terrace-config-core-jackson2}'s class of the same name, built on Jackson 3's
 * {@code tools.jackson.databind} instead — see that module's copy for the full rationale. One
 * {@link ObjectMapper}, built once through Jackson 3's builder (Jackson 3's mapper is immutable
 * once built, unlike Jackson 2's), is what makes two renders of the same {@link Contract} produce
 * the same bytes (see {@code spec/v1/FORMAT.md}'s "Publication").
 */
public final class ContractCodec {

    private static final ObjectMapper MAPPER = newMapper();

    private ContractCodec() {
    }

    private static ObjectMapper newMapper() {
        return JsonMapper.builder()
                .disable(SerializationFeature.FAIL_ON_EMPTY_BEANS)
                .disable(DeserializationFeature.FAIL_ON_UNKNOWN_PROPERTIES)
                .enable(SerializationFeature.INDENT_OUTPUT)
                .defaultPrettyPrinter(new CompactEmptyContainerPrettyPrinter())
                .build();
    }

    /** Renders {@code contract} as {@code json}, terminated with one trailing newline. */
    public static byte[] write(Contract contract) {
        String rendered = MAPPER.writeValueAsString(contract);
        return (rendered + "\n").getBytes(StandardCharsets.UTF_8);
    }

    public static void write(Contract contract, Path target) {
        try {
            Files.write(target, write(contract));
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    public static Contract read(byte[] json) {
        return MAPPER.readValue(json, Contract.class);
    }

    public static Contract read(Path source) {
        try (InputStream in = Files.newInputStream(source)) {
            return MAPPER.readValue(in, Contract.class);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }
}
