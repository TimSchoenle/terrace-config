package de.timscho.config.tck;

/**
 * The version string every corpus document carries in place of the real one.
 *
 * <p>Matches {@code CONFORMANCE_VERSION} in {@code rust/tests/spec.rs} exactly — the corpus is
 * one shared artefact, and a Java producer substituting a different sentinel would make every
 * stored case a two-language document instead of one. The producer's version moves on every
 * release and says nothing about the rendering, so leaving it in would make each release a
 * corpus-wide diff that hides the one line that mattered.
 */
public final class ConformanceVersion {

    public static final String VALUE = "0.0.0-conformance";

    private ConformanceVersion() {}
}
