package de.timscho.config.core.io;

import de.timscho.config.core.model.Contract;
import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import org.jetbrains.annotations.Blocking;
import org.jspecify.annotations.Nullable;
import tools.jackson.databind.DeserializationFeature;
import tools.jackson.databind.ObjectMapper;
import tools.jackson.databind.SerializationFeature;
import tools.jackson.databind.exc.MismatchedInputException;
import tools.jackson.databind.json.JsonMapper;

/**
 * Reads and writes a {@link Contract} as the {@code json} rendering: {@code
 * spec/v1/contract.schema.json}'s envelope, byte for byte.
 *
 * <p>Mirrors {@code terrace-config-core-jackson2}'s class of the same name, built on Jackson 3's
 * {@code tools.jackson.databind} instead; see that module's copy for the full rationale. One {@link
 * ObjectMapper}, built once through Jackson 3's builder (Jackson 3's mapper is immutable once
 * built, unlike Jackson 2's), is what makes two renders of the same {@link Contract} produce the
 * same bytes (see {@code spec/v1/FORMAT.md}'s "Publication").
 */
public final class ContractCodec {

    private static final ObjectMapper MAPPER = newMapper();

    private ContractCodec() {}

    private static ObjectMapper newMapper() {
        return JsonMapper.builder()
                .disable(SerializationFeature.FAIL_ON_EMPTY_BEANS)
                .disable(DeserializationFeature.FAIL_ON_UNKNOWN_PROPERTIES)
                .enable(SerializationFeature.INDENT_OUTPUT)
                .defaultPrettyPrinter(new CompactEmptyContainerPrettyPrinter())
                .build();
    }

    /**
     * Renders {@code contract} as {@code json}, terminated with one trailing newline.
     *
     * @param contract the contract to render
     * @return the rendered document, UTF-8 encoded
     */
    public static byte[] write(final Contract contract) {
        final String rendered = MAPPER.writeValueAsString(contract);
        return (rendered + "\n").getBytes(StandardCharsets.UTF_8);
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
     * spec/v1/contract.schema.json}. A document missing a required field binds with that field
     * {@code null}, or with its default where the model has one, such as {@code terrace_contract}.
     * Checking the bound result cannot tell an omitted field from a defaulted one, so a caller that
     * needs a valid envelope validates the document itself before binding it.
     *
     * @param json the document, UTF-8 encoded
     * @return the parsed contract
     * @throws tools.jackson.core.JacksonException if {@code json} is not JSON, is the JSON {@code
     *     null}, or does not bind to {@link Contract}
     */
    public static Contract read(final byte[] json) {
        return present(MAPPER.readValue(json, Contract.class));
    }

    /**
     * Parses the {@code json} rendering held in {@code source}. Binds without validating, as
     * {@link #read(byte[])} does.
     *
     * @param source the file to read
     * @return the parsed contract
     * @throws UncheckedIOException if the file cannot be read
     * @throws tools.jackson.core.JacksonException if the file is not JSON, is the JSON {@code null},
     *     or does not bind to {@link Contract}
     */
    @Blocking
    public static Contract read(final Path source) {
        try (InputStream in = Files.newInputStream(source)) {
            return present(MAPPER.readValue(in, Contract.class));
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    /**
     * The bound contract, refusing the JSON {@code null}: Jackson binds that document to {@code
     * null}, which a caller of this null-marked API would otherwise dereference.
     */
    private static Contract present(final @Nullable Contract contract) {
        if (contract == null) {
            throw MismatchedInputException.from(null, Contract.class, "the document is JSON null, not a contract");
        }
        return contract;
    }
}
