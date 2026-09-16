package de.timscho.config.tck.fixtures;

import java.util.List;

import com.fasterxml.jackson.annotation.JsonProperty;

import de.timscho.config.annotations.ElementValues;
import de.timscho.config.annotations.Nested;
import de.timscho.config.annotations.Range;
import de.timscho.config.annotations.TerraceConfig;
import de.timscho.config.annotations.Values;

/**
 * The {@code full-surface} spec case ({@code spec/v1/conformance/full-surface/}): every field
 * shape one struct can carry at once — a defaulted leaf, a required one, a bare enum, a bounded
 * float, a list of a foreign enum, and a nested struct with its own secret, alias and numeric
 * note. {@code FixtureContracts.fullSurface()} assembles this the same way {@code
 * terrace-config-example-service}'s {@code Config} assembles its own contract.
 */
@TerraceConfig
public final class FullSurfaceConfig {

    /** Bundle directory the readiness probe checks. */
    @JsonProperty("dist_dir")
    private String distDir = "public";

    /** Base URL every outbound request is resolved against. */
    @JsonProperty("api_base")
    private String apiBase;

    /** How much the service says. */
    @JsonProperty("log_level")
    @Values
    private LogLevel logLevel = LogLevel.INFO;

    /** Fraction of requests sampled for tracing. */
    @JsonProperty("sample_rate")
    @Range(min = 0.0, max = 1.0)
    private double sampleRate = 0.0;

    /** Methods every route admits. */
    @ElementValues({"GET", "POST"})
    private List<Method> methods = List.of();

    /** GitHub access this service authenticates outbound requests with. */
    @Nested
    private Github github = new Github();

    public String getDistDir() {
        return distDir;
    }

    public void setDistDir(final String distDir) {
        this.distDir = distDir;
    }

    public String getApiBase() {
        return apiBase;
    }

    public void setApiBase(final String apiBase) {
        this.apiBase = apiBase;
    }

    public LogLevel getLogLevel() {
        return logLevel;
    }

    public void setLogLevel(final LogLevel logLevel) {
        this.logLevel = logLevel;
    }

    public double getSampleRate() {
        return sampleRate;
    }

    public void setSampleRate(final double sampleRate) {
        this.sampleRate = sampleRate;
    }

    public List<Method> getMethods() {
        return methods;
    }

    public void setMethods(final List<Method> methods) {
        this.methods = methods;
    }

    public Github getGithub() {
        return github;
    }

    public void setGithub(final Github github) {
        this.github = github;
    }
}
