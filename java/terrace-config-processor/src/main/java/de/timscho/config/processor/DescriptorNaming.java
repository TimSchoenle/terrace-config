package de.timscho.config.processor;

import java.util.ArrayDeque;
import java.util.Deque;

import javax.lang.model.element.Element;
import javax.lang.model.element.TypeElement;
import javax.lang.model.util.Elements;

/**
 * The generated descriptor is always a top-level class, even for a {@code @TerraceConfig} type
 * nested inside another class — {@code javax.annotation.processing.Filer#createSourceFile} treats
 * every dot in the name it is given as a package separator, so a qualified name like {@code
 * test.Outer.Inner} cannot be turned into {@code test.Outer.InnerDescriptor} by simple
 * concatenation: that would ask the filer for a class named {@code InnerDescriptor} in a
 * (non-existent) package {@code test.Outer}.
 *
 * <p>Instead, every enclosing simple name is joined with {@code $} — the same character the JVM's
 * own binary class names use for nesting — into one top-level class living directly in the
 * annotated type's package.
 */
final class DescriptorNaming {

    private DescriptorNaming() {
    }

    static String simpleName(TypeElement type) {
        Deque<String> segments = new ArrayDeque<>();
        Element current = type;
        while (current instanceof TypeElement) {
            segments.addFirst(current.getSimpleName().toString());
            current = current.getEnclosingElement();
        }
        StringBuilder out = new StringBuilder();
        for (String segment : segments) {
            if (out.length() > 0) {
                out.append('$');
            }
            out.append(segment);
        }
        out.append("Descriptor");
        return out.toString();
    }

    static String qualifiedName(TypeElement type, Elements elements) {
        String pkg = elements.getPackageOf(type).getQualifiedName().toString();
        String simple = simpleName(type);
        return pkg.isEmpty() ? simple : pkg + "." + simple;
    }
}
