package de.timscho.config.tck;

import java.nio.file.Path;
import java.nio.file.Paths;

/**
 * Where {@code spec/v1/} lives, relative to this module.
 *
 * <p>Mirrors {@code spec_dir()} in {@code rust/tests/spec.rs}: both climb from a manifest
 * directory that is not the repository root to the one shared artefact. Here the manifest
 * directory is {@code java/terrace-config-spec-tck}, two levels below the root rather than one,
 * because {@code java/} itself is a sibling of {@code rust/}, not the module directory.
 */
final class SpecPaths {

    private SpecPaths() {}

    static Path specV1Dir() {
        return Paths.get("").toAbsolutePath().resolve("../../spec/v1").normalize();
    }

    static Path metaSchema() {
        return specV1Dir().resolve("contract.schema.json");
    }

    static Path conformanceCase(String name) {
        return specV1Dir().resolve("conformance").resolve(name).resolve("contract.json");
    }
}
