package de.timscho.config.processor;

import javax.lang.model.type.TypeKind;
import javax.lang.model.type.TypeMirror;

/**
 * The Java leaf spellings this processor reads without any annotation — the primitives, their
 * boxed forms, {@code String}, and the two arbitrary-precision number types. Mirrors the leaf set
 * {@code rust/docs/SCHEMA.md} reads off the built-in numeric and string types.
 */
final class LeafTypes {

    private LeafTypes() {
    }

    static boolean isLeaf(TypeMirror type) {
        if (type.getKind().isPrimitive()) {
            return true;
        }
        if (type.getKind() != TypeKind.DECLARED) {
            return false;
        }
        String name = type.toString();
        switch (name) {
            case "java.lang.String":
            case "java.lang.Boolean":
            case "java.lang.Byte":
            case "java.lang.Short":
            case "java.lang.Integer":
            case "java.lang.Long":
            case "java.lang.Float":
            case "java.lang.Double":
            case "java.lang.Character":
            case "java.math.BigInteger":
            case "java.math.BigDecimal":
                return true;
            default:
                return false;
        }
    }

    /** Whether a type is one {@code @Range} can bound — every leaf except {@code String}/{@code Character}/booleans. */
    static boolean isNumeric(TypeMirror type) {
        if (!isLeaf(type)) {
            return false;
        }
        String name = type.getKind().isPrimitive() ? boxedName(type) : type.toString();
        switch (name) {
            case "java.lang.String":
            case "java.lang.Boolean":
            case "java.lang.Character":
                return false;
            default:
                return true;
        }
    }

    private static String boxedName(TypeMirror type) {
        switch (type.getKind()) {
            case BOOLEAN:
                return "java.lang.Boolean";
            case BYTE:
                return "java.lang.Byte";
            case SHORT:
                return "java.lang.Short";
            case INT:
                return "java.lang.Integer";
            case LONG:
                return "java.lang.Long";
            case CHAR:
                return "java.lang.Character";
            case FLOAT:
                return "java.lang.Float";
            case DOUBLE:
                return "java.lang.Double";
            default:
                return type.toString();
        }
    }
}
