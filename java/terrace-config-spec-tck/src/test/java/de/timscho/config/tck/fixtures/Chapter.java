package de.timscho.config.tck.fixtures;

import de.timscho.config.annotations.TerraceConfig;
import java.util.Map;
import java.util.TreeMap;

/**
 * One chapter of a {@link HandbookConfig}: a heading and a text, each by locale. Both are open maps
 * of strings as far as any type can say; what they must hold is published as a refinement at a
 * position inside the handbook's {@code chapters} element schema.
 */
@TerraceConfig
public final class Chapter {

    /** Heading, by locale. */
    private Map<String, String> title = new TreeMap<>();

    /** Text, by locale. */
    private Map<String, String> body = new TreeMap<>();

    public Chapter() {}

    Chapter(final Map<String, String> title, final Map<String, String> body) {
        this.title = new TreeMap<>(title);
        this.body = new TreeMap<>(body);
    }

    public Map<String, String> getTitle() {
        return title;
    }

    public void setTitle(final Map<String, String> title) {
        this.title = title;
    }

    public Map<String, String> getBody() {
        return body;
    }

    public void setBody(final Map<String, String> body) {
        this.body = body;
    }
}
