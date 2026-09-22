package de.timscho.config.tck.fixtures;

import de.timscho.config.annotations.TerraceConfig;
import java.util.Map;
import java.util.TreeMap;

/**
 * The legal-pages library {@link RequiredEntriesConfig} mounts under {@code legal}. Both maps are
 * open maps of strings as far as any type can say; what they must hold is published as a
 * refinement.
 */
@TerraceConfig
public final class LegalPages {

    /** Legal documents, by the name the site links them under. */
    private Map<String, String> documents = new TreeMap<>();

    /** Footer links, by label. */
    private Map<String, String> links = new TreeMap<>(Map.of("home", "/"));

    public Map<String, String> getDocuments() {
        return documents;
    }

    public void setDocuments(final Map<String, String> documents) {
        this.documents = documents;
    }

    public Map<String, String> getLinks() {
        return links;
    }

    public void setLinks(final Map<String, String> links) {
        this.links = links;
    }
}
