package de.timscho.config.loader;

import java.io.IOException;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

import lombok.experimental.UtilityClass;

/**
 * Shared plumbing between the three file/environment-backed layers: reading a value file, and
 * inserting a value at a dot-separated key path in a nested {@link Map}.
 */
@UtilityClass
class LayerValues {

    /**
     * The contents of one value file, minus trailing line terminators.
     *
     * <p>Only {@code \r} and {@code \n} are stripped, never spaces or tabs: a text editor adding
     * a trailing newline is not part of the value, but a trailing space can be a real character
     * of a real password.
     */
    static String readValue(Path path) {
        byte[] bytes;
        try {
            bytes = Files.readAllBytes(path);
        } catch (IOException e) {
            throw new LoaderException("reading " + path + ": " + e.getMessage(), e);
        }
        String text = decodeStrictUtf8(path, bytes);
        int end = text.length();
        while (end > 0 && (text.charAt(end - 1) == '\r' || text.charAt(end - 1) == '\n')) {
            end--;
        }
        return text.substring(0, end);
    }

    private static String decodeStrictUtf8(Path path, byte[] bytes) {
        try {
            return StandardCharsets.UTF_8
                    .newDecoder()
                    .onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT)
                    .decode(java.nio.ByteBuffer.wrap(bytes))
                    .toString();
        } catch (CharacterCodingException e) {
            throw new LoaderException(path + " is not valid UTF-8", e);
        }
    }

    /**
     * Insert {@code value} at a dot-separated {@code key} path, creating intermediate maps. A
     * non-map found where a nested map is expected is replaced, matching what the environment
     * layer does when two keys disagree about whether a segment is a leaf.
     */
    @SuppressWarnings("unchecked")
    static void insertNested(Map<String, Object> dict, String key, Object value) {
        int dot = key.indexOf('.');
        if (dot < 0) {
            dict.put(key, value);
            return;
        }
        String head = key.substring(0, dot);
        String rest = key.substring(dot + 1);
        Object entry = dict.get(head);
        if (!(entry instanceof Map)) {
            entry = new LinkedHashMap<String, Object>();
            dict.put(head, entry);
        }
        insertNested((Map<String, Object>) entry, rest, value);
    }

    /**
     * Deep-merge {@code source} into {@code target}: a nested map merges recursively, anything
     * else overwrites.
     */
    @SuppressWarnings("unchecked")
    static void deepMerge(Map<String, Object> target, Map<String, Object> source) {
        for (Map.Entry<String, Object> entry : source.entrySet()) {
            Object existing = target.get(entry.getKey());
            Object incoming = entry.getValue();
            if (existing instanceof Map && incoming instanceof Map) {
                deepMerge((Map<String, Object>) existing, (Map<String, Object>) incoming);
            } else {
                target.put(entry.getKey(), incoming);
            }
        }
    }
}
