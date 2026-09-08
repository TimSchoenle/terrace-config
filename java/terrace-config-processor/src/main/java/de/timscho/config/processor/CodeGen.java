package de.timscho.config.processor;

import java.util.List;

/** Small helpers for rendering Java source literals into the generated descriptor classes. */
final class CodeGen {

    private CodeGen() {
    }

    static String stringLiteral(String value) {
        StringBuilder out = new StringBuilder("\"");
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '"':
                    out.append("\\\"");
                    break;
                case '\\':
                    out.append("\\\\");
                    break;
                case '\n':
                    out.append("\\n");
                    break;
                case '\r':
                    out.append("\\r");
                    break;
                case '\t':
                    out.append("\\t");
                    break;
                default:
                    out.append(c);
            }
        }
        out.append('"');
        return out.toString();
    }

    static String nullableStringLiteral(String value) {
        return value == null ? "null" : stringLiteral(value);
    }

    static String stringListLiteral(List<String> values) {
        if (values.isEmpty()) {
            return "java.util.List.of()";
        }
        StringBuilder out = new StringBuilder("java.util.List.of(");
        for (int i = 0; i < values.size(); i++) {
            if (i > 0) {
                out.append(", ");
            }
            out.append(stringLiteral(values.get(i)));
        }
        out.append(")");
        return out.toString();
    }

    static String doubleLiteral(Double value) {
        if (value == null) {
            return "null";
        }
        return "Double.valueOf(" + value + ")";
    }
}
