package de.timscho.config.processor;

/** Shortens a fully qualified type rendering to its simple-name form, for the descriptor's {@code typeName}. */
final class TypeNames {

    private TypeNames() {
    }

    static String simplify(String qualified) {
        StringBuilder out = new StringBuilder();
        StringBuilder segment = new StringBuilder();
        for (int i = 0; i <= qualified.length(); i++) {
            char c = i < qualified.length() ? qualified.charAt(i) : 0;
            boolean boundary = i == qualified.length() || c == '<' || c == '>' || c == ',' || c == ' ';
            if (boundary) {
                out.append(lastSegment(segment.toString()));
                segment.setLength(0);
                if (i < qualified.length()) {
                    out.append(c);
                }
            } else {
                segment.append(c);
            }
        }
        return out.toString();
    }

    private static String lastSegment(String dotted) {
        int idx = dotted.lastIndexOf('.');
        return idx < 0 ? dotted : dotted.substring(idx + 1);
    }
}
