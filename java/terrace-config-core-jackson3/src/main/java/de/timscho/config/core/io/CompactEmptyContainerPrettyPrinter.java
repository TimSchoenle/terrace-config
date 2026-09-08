package de.timscho.config.core.io;

import tools.jackson.core.JsonGenerator;
import tools.jackson.core.util.DefaultIndenter;
import tools.jackson.core.util.DefaultPrettyPrinter;

/**
 * {@link DefaultPrettyPrinter} indented with two spaces, except that an empty array or object is
 * rendered as {@code []} / {@code {}} rather than Jackson's default {@code [ ]} / {@code { }}.
 *
 * <p>Mirrors {@code terrace-config-core-jackson2}'s class of the same name, field for field,
 * built on Jackson 3's {@code tools.jackson.core} instead. See that module's copy for the full
 * rationale — the corpus under {@code spec/v1/conformance/} spells an empty array without the
 * inner space, and publication is defined as byte-stable in {@code spec/v1/FORMAT.md}.
 */
final class CompactEmptyContainerPrettyPrinter extends DefaultPrettyPrinter {

    CompactEmptyContainerPrettyPrinter() {
        DefaultIndenter indenter = new DefaultIndenter("  ", "\n");
        indentObjectsWith(indenter);
        indentArraysWith(indenter);
    }

    private CompactEmptyContainerPrettyPrinter(CompactEmptyContainerPrettyPrinter base) {
        super(base);
    }

    @Override
    public DefaultPrettyPrinter createInstance() {
        return new CompactEmptyContainerPrettyPrinter(this);
    }

    /**
     * {@code "key": value}, not {@code "key" : value} — the corpus never has a space before the
     * colon. Named {@code writeObjectNameValueSeparator} here (Jackson 3 renamed the method from
     * Jackson 2's {@code writeObjectFieldValueSeparator}, field -&gt; name).
     */
    @Override
    public void writeObjectNameValueSeparator(JsonGenerator g) {
        g.writeRaw(": ");
    }

    @Override
    public void writeEndArray(JsonGenerator g, int nrOfEntries) {
        if (!_arrayIndenter.isInline()) {
            --_nesting;
        }
        if (nrOfEntries > 0) {
            _arrayIndenter.writeIndentation(g, _nesting);
        }
        g.writeRaw(']');
    }

    @Override
    public void writeEndObject(JsonGenerator g, int nrOfEntries) {
        if (!_objectIndenter.isInline()) {
            --_nesting;
        }
        if (nrOfEntries > 0) {
            _objectIndenter.writeIndentation(g, _nesting);
        }
        g.writeRaw('}');
    }
}
