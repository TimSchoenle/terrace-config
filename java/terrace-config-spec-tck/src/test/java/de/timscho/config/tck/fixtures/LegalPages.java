package de.timscho.config.tck.fixtures;

import de.timscho.config.annotations.Element;
import de.timscho.config.annotations.TerraceConfig;
import java.util.Map;
import java.util.TreeMap;

/**
 * The legal-pages library {@link RequiredEntriesConfig} mounts under {@code legal}. Both maps are
 * open as far as any type can say — any document name, any link label; which entries they must
 * hold, and how those are named, is published as a refinement.
 */
@TerraceConfig
public final class LegalPages {

    /** Legal documents, by the name the site links them under. */
    @Element
    private Map<String, LegalDocument> documents = new TreeMap<>();

    /** Footer links, by label. */
    private Map<String, String> links = new TreeMap<>(Map.of("home", "/"));

    public Map<String, LegalDocument> getDocuments() {
        return documents;
    }

    public void setDocuments(final Map<String, LegalDocument> documents) {
        this.documents = documents;
    }

    public Map<String, String> getLinks() {
        return links;
    }

    public void setLinks(final Map<String, String> links) {
        this.links = links;
    }
}
