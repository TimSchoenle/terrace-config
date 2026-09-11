package de.timscho.config.processor;

import javax.lang.model.type.TypeKind;
import javax.lang.model.type.TypeMirror;

/**
 * The Java leaf spellings this processor reads without any annotation — the primitives, their
 * boxed forms, {@code String}, and the two arbitrary-precision number types. Mirrors the leaf set
 * {@code rust/docs/SCHEMA.md} reads off the built-in numeric and string types.
 */
final class LeafTypes {

    private LeafTypes() {}

    static boolean isLeaf(TypeMirror type) {
        if (type.getKind().isPrimitive()) {
            return true;
        }
        if (type.getKind() != TypeKind.DECLARED) {
            return false;
        }
        String name = type.toString();
        return switch (name) {
            case "java.lang.String",
                    "java.lang.Boolean",
                    "java.lang.Byte",
                    "java.lang.Short",
                    "java.lang.Integer",
                    "java.lang.Long",
                    "java.lang.Float",
                    "java.lang.Double",
                    "java.lang.Character",
                    "java.math.BigInteger",
                    "java.math.BigDecimal" -> true;
            default -> false;
        };
    }

    /** Whether a type is one {@code @Range} can bound — every leaf except {@code String}/{@code Character}/booleans. */
    static boolean isNumeric(TypeMirror type) {
        if (!isLeaf(type)) {
            return false;
        }
        String name = type.getKind().isPrimitive() ? boxedName(type) : type.toString();
        return switch (name) {
            case "java.lang.String", "java.lang.Boolean", "java.lang.Character" -> false;
            default -> true;
        };
    }

    private static String boxedName(TypeMirror type) {
        return switch (type.getKind()) {
            case BOOLEAN -> "java.lang.Boolean";
            case BYTE -> "java.lang.Byte";
            case SHORT -> "java.lang.Short";
            case INT -> "java.lang.Integer";
            case LONG -> "java.lang.Long";
            case CHAR -> "java.lang.Character";
            case FLOAT -> "java.lang.Float";
            case DOUBLE -> "java.lang.Double";
            default -> type.toString();
        };
    }
}
