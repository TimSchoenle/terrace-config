// The annotation vocabulary a service's own types carry — @TerraceConfig and friends. Depends on
// nothing but the JDK: a service annotating its configuration types needs these on its compile
// classpath and nothing else, not the document model or the processor that reads them.
description = "@TerraceConfig and the rest of the annotation vocabulary read by " +
    "terrace-config-processor. No dependency beyond the JDK, by design."

// Only the plain conventions (Java toolchain, JUnit/AssertJ), not terrace-config.lombok-
// conventions: annotation types (`@interface`) can't carry Lombok annotations at all, and adding
// the plugin would put `lombok` (a compile-time-only jar, admittedly) on this module's classpath
// for no reason — undermining the "nothing but the JDK" guarantee this module exists to make.
// See java/README.md's Lombok section.
plugins {
    id("terrace-config.java-conventions")
}
