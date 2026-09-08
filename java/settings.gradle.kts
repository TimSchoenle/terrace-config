// Aggregator only — no code of its own. Building it builds every module in dependency order;
// releasing it releases nothing, since each artefact below is versioned and released
// independently (see release-please-config.json once the Java packages are wired in).
rootProject.name = "terrace-config-parent"

// The version catalog at gradle/libs.versions.toml is picked up by convention; no explicit
// dependencyResolutionManagement block needed.
include(
    "terrace-config-annotations",
    "terrace-config-core",
    "terrace-config-processor",
    "terrace-config-loader",
    "terrace-config-spring-boot",
    "terrace-config-spec-tck",
)
