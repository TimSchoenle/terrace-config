package de.timscho.config.core.io;

import static org.assertj.core.api.Assertions.assertThatThrownBy;

import com.fasterxml.jackson.databind.exc.MismatchedInputException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/** What {@link ContractCodec} refuses rather than binding. */
class ContractCodecTest {

    private static final byte[] JSON_NULL = "null".getBytes(StandardCharsets.UTF_8);

    @Test
    void theJsonNullIsRefusedRatherThanReturnedAsNull() {
        assertThatThrownBy(() -> ContractCodec.read(JSON_NULL))
                .isInstanceOf(UncheckedIOException.class)
                .hasCauseInstanceOf(MismatchedInputException.class);
    }

    @Test
    void aFileHoldingTheJsonNullIsRefusedToo(@TempDir final Path dir) throws Exception {
        final Path source = Files.write(dir.resolve("contract.json"), JSON_NULL);

        assertThatThrownBy(() -> ContractCodec.read(source))
                .isInstanceOf(UncheckedIOException.class)
                .hasCauseInstanceOf(MismatchedInputException.class);
    }
}
