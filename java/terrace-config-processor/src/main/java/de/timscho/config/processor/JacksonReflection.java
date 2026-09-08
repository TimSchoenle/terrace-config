package de.timscho.config.processor;

import javax.lang.model.element.AnnotationMirror;
import javax.lang.model.element.AnnotationValue;
import javax.lang.model.element.Element;
import javax.lang.model.element.ExecutableElement;
import java.util.Map;

/**
 * Reads {@code com.fasterxml.jackson.annotation.*} annotations by qualified name off {@link
 * AnnotationMirror} rather than by importing the annotation classes, so this processor takes no
 * compile dependency on either Jackson major — {@code jackson-annotations} is the one package
 * both majors share (see {@code docs/migration-progress.md}, "Dual Jackson 2 / 3 support"), and
 * even that is avoided here since an annotated service may compile against neither Jackson major
 * directly at the point the processor runs.
 */
final class JacksonReflection {

    private static final String JSON_IGNORE_PROPERTIES = "com.fasterxml.jackson.annotation.JsonIgnoreProperties";
    private static final String JSON_PROPERTY = "com.fasterxml.jackson.annotation.JsonProperty";

    private JacksonReflection() {
    }

    /** Whether {@code @JsonIgnoreProperties(ignoreUnknown = false)} is present. */
    static boolean isClosed(Element element) {
        for (AnnotationMirror mirror : element.getAnnotationMirrors()) {
            if (!mirror.getAnnotationType().toString().equals(JSON_IGNORE_PROPERTIES)) {
                continue;
            }
            for (Map.Entry<? extends ExecutableElement, ? extends AnnotationValue> entry
                    : mirror.getElementValues().entrySet()) {
                if (entry.getKey().getSimpleName().contentEquals("ignoreUnknown")) {
                    return Boolean.FALSE.equals(entry.getValue().getValue());
                }
            }
        }
        return false;
    }

    /** {@code @JsonProperty("...")}'s value, or {@code fallback} if the annotation is absent. */
    static String jsonPropertyName(Element element, String fallback) {
        for (AnnotationMirror mirror : element.getAnnotationMirrors()) {
            if (!mirror.getAnnotationType().toString().equals(JSON_PROPERTY)) {
                continue;
            }
            for (Map.Entry<? extends ExecutableElement, ? extends AnnotationValue> entry
                    : mirror.getElementValues().entrySet()) {
                if (entry.getKey().getSimpleName().contentEquals("value")) {
                    return String.valueOf(entry.getValue().getValue());
                }
            }
        }
        return fallback;
    }
}
