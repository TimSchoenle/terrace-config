package de.timscho.config.processor;

import javax.lang.model.type.DeclaredType;
import javax.lang.model.type.TypeMirror;
import java.util.List;

import de.timscho.config.core.descriptor.KeyDescriptor.ContainerKind;

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

    private ContainerShape(ContainerKind kind, @Nullable TypeMirror element) {
        this.kind = kind;
        this.element = element;
    }

    ContainerKind kind() {
        return kind;
    }

    @Nullable TypeMirror element() {
        return element;
    }

    static ContainerShape of(TypeMirror type) {
        if (!(type instanceof DeclaredType declared)) {
            return NONE;
        }
        String erasedName = declared.asElement().toString();
        List<? extends TypeMirror> args = declared.getTypeArguments();
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
    }
}
