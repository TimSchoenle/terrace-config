package de.timscho.config.example.service;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

import org.junit.jupiter.api.Test;

/**
 * {@code contract.json} is a checked-in file, not a build artefact: nothing regenerates it
 * automatically, so nothing but this test stops it from drifting out from under {@link Config}'s
 * actual shape the moment a field changes here without {@code --contract} being re-run by hand.
 * {@code ./gradlew test} already runs in CI on every push, which is what makes this the same
 * guarantee {@code spotlessCheck} gives formatting — checked fresh, every time, never trusted
 * merely because it once matched. The Rust counterpart is
 * {@code tests/example_service.rs}'s {@code the_checked_in_contract_matches_what_the_types_render_today}.
 */
class ContractTest {

    @Test
    void theCheckedInContractMatchesWhatTheTypesRenderToday() throws IOException {
        String rendered = ContractGenerator.toJson(ContractGenerator.generate());
        String checkedIn = Files.readString(Path.of("contract.json"), StandardCharsets.UTF_8);

        assertThat(rendered.strip())
                .as("contract.json is stale -- regenerate it with: "
                        + "./gradlew :terrace-config-example-service:run --args=--contract > "
                        + "terrace-config-example-service/contract.json")
                .isEqualTo(checkedIn.strip());
    }
}
