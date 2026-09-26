package de.timscho.config.tck.fixtures;

import com.fasterxml.jackson.annotation.JsonProperty;
import de.timscho.config.annotations.Element;
import de.timscho.config.annotations.TerraceConfig;
import java.util.Map;
import java.util.TreeMap;

/**
 * The {@code element-patterns} spec case ({@code spec/v1/conformance/element-patterns/}): a
 * handbook whose chapters are each a map of locale to text, twice over. {@code
 * FixtureContracts.elementPatterns()} refines positions inside {@code chapters}' element schema,
 * which exists only because {@link Chapter} is reported as the map's {@code @Element}.
 */
@TerraceConfig
public final class HandbookConfig {

    /** The locale a chapter falls back to. */
    @JsonProperty("default_locale")
    private String defaultLocale = "en";

    /** Chapters, by slug. */
    @Element
    private Map<String, Chapter> chapters =
            new TreeMap<>(Map.of("intro", new Chapter(Map.of("en", "Introduction"), Map.of("en", "Start here."))));

    public String getDefaultLocale() {
        return defaultLocale;
    }

    public void setDefaultLocale(final String defaultLocale) {
        this.defaultLocale = defaultLocale;
    }

    public Map<String, Chapter> getChapters() {
        return chapters;
    }

    public void setChapters(final Map<String, Chapter> chapters) {
        this.chapters = chapters;
    }
}
