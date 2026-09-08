package de.timscho.config.processor;

import javax.lang.model.element.Element;

/**
 * Raised when a field's shape cannot be resolved — the compile error {@code rust/docs/SCHEMA.md}
 * calls "the whole point of the feature". Carries the offending {@link Element} so {@link
 * TerraceConfigProcessor} can report it at the right source position.
 */
final class DescriptorException extends RuntimeException {

    private final transient Element element;

    DescriptorException(Element element, String message) {
        super(message);
        this.element = element;
    }

    Element element() {
        return element;
    }
}
