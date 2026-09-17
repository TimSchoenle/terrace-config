package de.timscho.config.processor;

import de.timscho.config.core.descriptor.KeyDescriptor.ContainerKind;
import java.util.List;
import javax.lang.model.type.DeclaredType;
import javax.lang.model.type.TypeMirror;
import org.jspecify.annotations.Nullable;

/**
 * The result of asking whether a field's declared type is one of the containers this processor
 * looks through — {@code Optional}, {@code List}, {@code Set} or {@code Map} — and, if so, what
 * its element type is. A map's *key* type is not reported: a TOML table's keys are strings
 * whatever the map is keyed by, mirroring {@code rust/docs/SCHEMA.md}, "A key that holds many of
 * something".
 */
final class ContainerShape {

    static final ContainerShape NONE = new ContainerShape(ContainerKind.NONE, null);

    private final ContainerKind kind;
    private final @Nullable TypeMirror element;

    private ContainerShape(final ContainerKind kind, @Nullable final TypeMirror element) {
        this.kind = kind;
        this.element = element;
    }

    ContainerKind kind() {
        return this.kind;
    }

    @Nullable TypeMirror element() {
        return this.element;
    }

    static ContainerShape of(final TypeMirror type) {
        if (!(type instanceof DeclaredType declared)) {
            return NONE;
        }
        final String erasedName = declared.asElement().toString();
        final List<? extends TypeMirror> args = declared.getTypeArguments();
        // CHECKSTYLE.OFF: Indentation -- palantirJavaFormat wraps each arrow-case body at 4 spaces
        // past `case`; the fetched ruleset's Indentation check wants 8. Reformatting by hand would
        // just be undone by the next spotlessApply, so this scoped disable defers to the formatter
        // that actually governs this file (see terrace-config.java-conventions.gradle.kts'
        // checkstyle block).
        return switch (erasedName) {
            case "java.util.Optional" ->
                args.size() == 1 ? new ContainerShape(ContainerKind.OPTIONAL, args.get(0)) : NONE;
            case "java.util.List", "java.util.ArrayList" ->
                args.size() == 1 ? new ContainerShape(ContainerKind.LIST, args.get(0)) : NONE;
            case "java.util.Set", "java.util.HashSet", "java.util.SortedSet" ->
                args.size() == 1 ? new ContainerShape(ContainerKind.SET, args.get(0)) : NONE;
            case "java.util.Map", "java.util.HashMap", "java.util.SortedMap" ->
                // The value, not the key — see class Javadoc.
                args.size() == 2 ? new ContainerShape(ContainerKind.MAP, args.get(1)) : NONE;
            default -> NONE;
        };
        // CHECKSTYLE.ON: Indentation
    }
}
