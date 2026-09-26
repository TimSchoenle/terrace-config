package de.timscho.config.tck.fixtures;

import de.timscho.config.annotations.TerraceConfig;

/** One page {@link LegalPages} links to: a heading the site cannot render without, and a body. */
@TerraceConfig
public final class LegalDocument {

    /** Heading of the page. */
    private String title;

    /** Markdown body. */
    private String body = "";

    public String getTitle() {
        return title;
    }

    public void setTitle(final String title) {
        this.title = title;
    }

    public String getBody() {
        return body;
    }

    public void setBody(final String body) {
        this.body = body;
    }
}
