package de.timscho.config.core.io;

import java.io.IOException;

import com.fasterxml.jackson.core.JsonGenerator;
import com.fasterxml.jackson.core.util.DefaultIndenter;
import com.fasterxml.jackson.core.util.DefaultPrettyPrinter;

/**
 * {@link DefaultPrettyPrinter} indented with two spaces, except that an empty array or object is
 * rendered as {@code []} / {@code {}} rather than Jackson's default {@code [ ]} / {@code { }}.
 *
 * <p>The corpus under {@code spec/v1/conformance/} — and every hand-written {@code contract.json}
 * — spells an empty array without the inner space (see {@code "values": []} in {@code
 * spec/v1/conformance/minimal/contract.json}). Publication is defined as byte-stable in {@code
 * spec/v1/FORMAT.md}, so matching this exactly is not cosmetic: it is what lets a re-rendered
 * document be compared byte for byte against a stored one.
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

    /** {@code "key": value}, not {@code "key" : value} — the corpus never has a space before the colon. */
    @Override
    public void writeObjectFieldValueSeparator(JsonGenerator g) throws IOException {
        g.writeRaw(": ");
    }

    @Override
    public void writeEndArray(JsonGenerator g, int nrOfEntries) throws IOException {
        if (!_arrayIndenter.isInline()) {
            --_nesting;
        }
        if (nrOfEntries > 0) {
            _arrayIndenter.writeIndentation(g, _nesting);
        }
        g.writeRaw(']');
    }

    @Override
    public void writeEndObject(JsonGenerator g, int nrOfEntries) throws IOException {
        if (!_objectIndenter.isInline()) {
            --_nesting;
        }
        if (nrOfEntries > 0) {
            _objectIndenter.writeIndentation(g, _nesting);
        }
        g.writeRaw('}');
    }
}
