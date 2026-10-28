package de.timscho.config.core.io;

import com.fasterxml.jackson.databind.DeserializationFeature;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.SerializationFeature;
import de.timscho.config.core.model.Contract;
import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import org.jetbrains.annotations.Blocking;

/**
 * Reads and writes a {@link Contract} as the {@code json} rendering: {@code
 * spec/v1/contract.schema.json}'s envelope, byte for byte.
 *
 * <p>One {@link ObjectMapper}, configured once, rather than one built ad hoc per call site: the
 * pretty printer and the inclusion rules are what make two renders of the same {@link Contract}
 * produce the same bytes (see {@code spec/v1/FORMAT.md}'s "Publication"), and that guarantee only
 * holds if every call site shares it.
 */
public final class ContractCodec {

    private static final ObjectMapper MAPPER = newMapper();

    private ContractCodec() {}

    private static ObjectMapper newMapper() {
        final ObjectMapper mapper = new ObjectMapper();
        mapper.disable(SerializationFeature.FAIL_ON_EMPTY_BEANS);
        mapper.disable(DeserializationFeature.FAIL_ON_UNKNOWN_PROPERTIES);
        mapper.enable(SerializationFeature.INDENT_OUTPUT);
        mapper.setDefaultPrettyPrinter(new CompactEmptyContainerPrettyPrinter());
        return mapper;
    }

    /**
     * Renders {@code contract} as {@code json}, terminated with one trailing newline.
     *
     * @param contract the contract to render
     * @return the rendered document, UTF-8 encoded
     */
    public static byte[] write(final Contract contract) {
        try {
            final String rendered = MAPPER.writeValueAsString(contract);
            return (rendered + "\n").getBytes(StandardCharsets.UTF_8);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    /**
     * Renders {@code contract} as {@code json} into {@code target}, replacing whatever it held.
     *
     * @param contract the contract to render
     * @param target   the file to write
     * @throws UncheckedIOException if the file cannot be written
     */
    @Blocking
    public static void write(final Contract contract, final Path target) {
        try {
            Files.write(target, write(contract));
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    /**
     * Parses a {@code json} rendering. Unknown properties are ignored.
     *
     * <p>Binding is not validation: nothing here checks the document against {@code
     * spec/v1/contract.schema.json}, so a document missing a required field binds with that field
     * {@code null}. A caller that needs a valid envelope validates the result.
     *
     * @param json the document, UTF-8 encoded
     * @return the parsed contract
     * @throws UncheckedIOException if {@code json} is not JSON or does not bind to {@link Contract}
     */
    public static Contract read(final byte[] json) {
        try {
            return MAPPER.readValue(json, Contract.class);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    /**
     * Parses the {@code json} rendering held in {@code source}. Binds without validating, as
     * {@link #read(byte[])} does.
     *
     * @param source the file to read
     * @return the parsed contract
     * @throws UncheckedIOException if the file cannot be read, is not JSON, or does not bind to
     *     {@link Contract}
     */
    @Blocking
    public static Contract read(final Path source) {
        try (InputStream in = Files.newInputStream(source)) {
            return MAPPER.readValue(in, Contract.class);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }
}
