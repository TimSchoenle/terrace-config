package de.timscho.config.core.io;

import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

import com.fasterxml.jackson.databind.DeserializationFeature;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.SerializationFeature;

import de.timscho.config.core.model.Contract;

import org.jetbrains.annotations.Blocking;

/**
 * Reads and writes a {@link Contract} as the {@code json} rendering — {@code
 * spec/v1/contract.schema.json}'s envelope, byte for byte.
 *
 * <p>One {@link ObjectMapper}, configured once, rather than one built ad hoc per call site: the
 * pretty printer and the inclusion rules are what make two renders of the same {@link Contract}
 * produce the same bytes (see {@code spec/v1/FORMAT.md}'s "Publication"), and that guarantee only
 * holds if every call site shares it.
 */
public final class ContractCodec {

    private static final ObjectMapper MAPPER = newMapper();

    private ContractCodec() {
    }

    private static ObjectMapper newMapper() {
        ObjectMapper mapper = new ObjectMapper();
        mapper.disable(SerializationFeature.FAIL_ON_EMPTY_BEANS);
        mapper.disable(DeserializationFeature.FAIL_ON_UNKNOWN_PROPERTIES);
        mapper.enable(SerializationFeature.INDENT_OUTPUT);
        mapper.setDefaultPrettyPrinter(new CompactEmptyContainerPrettyPrinter());
        return mapper;
    }

    /** Renders {@code contract} as {@code json}, terminated with one trailing newline. */
    public static byte[] write(Contract contract) {
        try {
            String rendered = MAPPER.writeValueAsString(contract);
            return (rendered + "\n").getBytes(StandardCharsets.UTF_8);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    @Blocking
    public static void write(Contract contract, Path target) {
        try {
            Files.write(target, write(contract));
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    public static Contract read(byte[] json) {
        try {
            return MAPPER.readValue(json, Contract.class);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    @Blocking
    public static Contract read(Path source) {
        try (InputStream in = Files.newInputStream(source)) {
            return MAPPER.readValue(in, Contract.class);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }
}
