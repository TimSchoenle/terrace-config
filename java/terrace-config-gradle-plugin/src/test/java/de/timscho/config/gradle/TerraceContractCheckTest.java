package de.timscho.config.gradle;

import static org.assertj.core.api.Assertions.assertThat;

import java.nio.charset.StandardCharsets;
import org.junit.jupiter.api.Test;

class TerraceContractCheckTest {

    @Test
    void namesTheFirstLineThatDiffers() {
        assertThat(TerraceContractCheck.firstDifference(bytes("a\nb\nc\n"), bytes("a\nB\nc\n")))
                .contains("first at line 2")
                .contains("committed: b")
                .contains("emitted:   B");
    }

    @Test
    void namesTheEndOfTheShorterDocument() {
        assertThat(TerraceContractCheck.firstDifference(bytes("a\n"), bytes("a\nb\n")))
                .contains("first at line 2")
                .contains("committed: <end of file>")
                .contains("emitted:   b");
    }

    @Test
    void saysWhenOnlyLineEndingsDiffer() {
        assertThat(TerraceContractCheck.firstDifference(bytes("a\r\nb\r\n"), bytes("a\nb\n")))
                .isEqualTo(", differing only in line endings or a final newline");
    }

    private static byte[] bytes(final String text) {
        return text.getBytes(StandardCharsets.UTF_8);
    }
}
