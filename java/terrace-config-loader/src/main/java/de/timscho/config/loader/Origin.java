package de.timscho.config.loader;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/**
 * One key, and every layer that supplied it.
 *
 * <p>Constructed only through {@link #fromSources}, which keeps the same invariant the Rust
 * crate's {@code Origin} keeps by construction: a key is here <i>because</i> some layer supplied
 * it, so there is always exactly one {@link #effective()} layer.
 */
public final class Origin {

    private final String key;
    private final Layer effective;
    private final List<Layer> shadowed;

    private Origin(String key, Layer effective, List<Layer> shadowed) {
        this.key = key;
        this.effective = effective;
        this.shadowed = shadowed;
    }

    /** The key path, e.g. {@code auth.jwt_secret}. */
    public String key() {
        return key;
    }

    /** The layer whose value is in effect: the last one merged. */
    public Layer effective() {
        return effective;
    }

    /**
     * The layers that also supplied this key and lost, lowest precedence first.
     *
     * <p>Empty for almost every key. When it is not, it is the answer to "why is my mounted
     * secret not being picked up" — the mount is right there in the list, underneath whatever
     * beat it.
     */
    public List<Layer> shadowed() {
        return shadowed;
    }

    /** Every layer that supplied this key, lowest precedence first, ending with {@link #effective()}. */
    public List<Layer> sources() {
        List<Layer> all = new ArrayList<>(shadowed);
        all.add(effective);
        return all;
    }

    /** Whether more than one layer supplied this key. */
    public boolean isContested() {
        return !shadowed.isEmpty();
    }

    /**
     * One key's sources, in merge order, split into the one in effect and the rest. {@code null}
     * for an empty list, which cannot happen in practice — an entry exists because a layer wrote
     * into it.
     */
    static Origin fromSources(String key, List<Layer> sources) {
        if (sources.isEmpty()) {
            return null;
        }
        List<Layer> shadowed = Collections.unmodifiableList(new ArrayList<>(sources.subList(0, sources.size() - 1)));
        Layer effective = sources.get(sources.size() - 1);
        return new Origin(key, effective, shadowed);
    }
}
