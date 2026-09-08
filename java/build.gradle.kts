// The Java implementations of the terrace configuration contract (../spec/v1/). Shared build
// configuration lives in buildSrc (see java/buildSrc/src/main/kotlin/terrace-config.*-conventions
// .gradle.kts), applied per-module instead of via a root `subprojects{}` block — see
// java/README.md for what each module is, which tier it targets, and why buildSrc.

allprojects {
    group = "de.timscho"
    version = "0.1.0-SNAPSHOT"
}
