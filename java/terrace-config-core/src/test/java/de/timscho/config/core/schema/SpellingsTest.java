package de.timscho.config.core.schema;

import static org.assertj.core.api.Assertions.assertThat;

import de.timscho.config.core.model.Dialect;
import org.junit.jupiter.api.Test;

/** The spelling rules the Rust crate's {@code schema::tests} pin, pinned the same way here. */
class SpellingsTest {

    private static Dialect dialect(final String separator) {
        return Dialect.builder()
                .prefix("TEST_")
                .nestingSeparator(separator)
                .indirectionSuffix("_FILE")
                .build();
    }

    @Test
    void anIndirectionVariableReachingItsKeyIsPublished() {
        assertThat(Spellings.indirectionName(dialect("__"), "TEST_UNIT__FILENAME", "unit.filename"))
                .isEqualTo("TEST_UNIT__FILENAME_FILE");
    }

    /**
     * The environment layer splits on the separator before folding case and the indirection layer
     * folds first, so a separator that is a letter of the key reaches the key through the plain
     * variable and somewhere else through its {@code _FILE} twin. Found by the Rust crate's
     * {@code schema} fuzz target.
     */
    @Test
    void anIndirectionVariableTheIndirectionLayerMisreadsIsNotPublished() {
        final Dialect dialect = dialect("e");
        assertThat(Spellings.envSpelling(dialect, "filename.file").getEnv()).isEqualTo("TEST_FILENAMEeFILE");
        assertThat(Spellings.indirectionName(dialect, "TEST_FILENAMEeFILE", "filename.file"))
                .isNull();
    }
}
