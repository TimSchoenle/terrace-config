package de.timscho.config.tck;

import java.util.ArrayList;
import java.util.Iterator;
import java.util.List;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import org.jspecify.annotations.Nullable;

/**
 * Compares a produced document against a stored corpus case, at the level the claimed tier
 * covers — no more.
 *
 * <p>The output is a readable diff, one entry per disagreement, never an assertion message: the
 * diff is the deliverable when a rendering changes, not an afterthought a failure message happens
 * to carry.
 */
public final class TierComparator {

    /** The tier-2 fields compared field-for-field, for every key, in the order they are checked. */
    private static final String[] TIER_2_FIELDS = {
        "env", "env_file", "secrets_file",
        "env_aliases", "env_file_aliases", "secrets_file_aliases",
        "unreachable"
    };

    private TierComparator() {}

    /**
     * Every disagreement between {@code produced} and {@code expected} that the given tier cares
     * about. Empty means the produced document satisfies the claimed tier against this case.
     */
    public static List<String> compare(final Tier tier, final JsonNode produced, final JsonNode expected) {
        final List<String> diffs = new ArrayList<>();
        compareTier1(produced, diffs);
        if (tier == Tier.TIER_1) {
            return diffs;
        }
        compareTier2(produced, expected, diffs);
        if (tier == Tier.TIER_2) {
            return diffs;
        }
        compareTier3(produced, expected, diffs);
        return diffs;
    }

    private static void compareTier1(final JsonNode produced, final List<String> diffs) {
        final JsonNode producer = produced.path("producer");
        for (String field : new String[] {"name", "version", "loader"}) {
            final String value = producer.path(field).asText("");
            if (value.isBlank()) {
                diffs.add("tier 1: `producer." + field + "` is missing or blank");
            }
        }
    }

    private static void compareTier2(final JsonNode produced, final JsonNode expected, final List<String> diffs) {
        for (JsonNode expectedKey : expected.path("schema").path("keys")) {
            final String path = expectedKey.path("path").asText(null);
            if (path == null) {
                continue; // Malformed corpus; the meta-schema check reports this separately.
            }
            final JsonNode producedKey = findKey(produced, path);
            if (producedKey == null) {
                diffs.add("tier 2: key `" + path + "` is in the stored case but not in the produced document");
                continue;
            }
            for (String field : TIER_2_FIELDS) {
                final JsonNode expectedValue = expectedKey.path(field);
                final JsonNode producedValue = producedKey.path(field);
                if (!expectedValue.equals(producedValue)) {
                    diffs.add("tier 2: key `" + path + "`, field `" + field + "`: expected " + expectedValue
                            + ", produced " + producedValue);
                }
            }
        }
    }

    private static @Nullable JsonNode findKey(final JsonNode document, final String path) {
        for (JsonNode key : document.path("schema").path("keys")) {
            if (path.equals(key.path("path").asText(null))) {
                return key;
            }
        }
        return null;
    }

    private static void compareTier3(final JsonNode produced, final JsonNode expected, final List<String> diffs) {
        final JsonNode normalisedProduced = withConformanceVersion(produced);
        final JsonNode normalisedExpected = withConformanceVersion(expected);
        if (!normalisedProduced.equals(normalisedExpected)) {
            diffs.add("tier 3: " + firstDifference(normalisedExpected, normalisedProduced));
        }
    }

    /** {@code producer.version} substituted with {@link ConformanceVersion#VALUE} on a copy. */
    private static JsonNode withConformanceVersion(final JsonNode document) {
        final JsonNode copy = document.deepCopy();
        final JsonNode producer = copy.path("producer");
        if (producer instanceof ObjectNode producerObject) {
            producerObject.put("version", ConformanceVersion.VALUE);
        }
        return copy;
    }

    /**
     * The first path the two documents disagree on. A whole-document dump is not useful to a
     * reader chasing one changed field, which is the common case once a rendering is close.
     */
    private static String firstDifference(final JsonNode expected, final JsonNode produced) {
        return firstDifference("", expected, produced);
    }

    private static String firstDifference(final String path, final JsonNode expected, final JsonNode produced) {
        if (expected.isObject() && produced.isObject()) {
            final Iterator<String> names = expected.fieldNames();
            while (names.hasNext()) {
                final String name = names.next();
                final String childPath = path + "/" + name;
                if (!produced.has(name)) {
                    return "at `" + childPath + "`: missing from the produced document";
                }
                if (!expected.get(name).equals(produced.get(name))) {
                    return firstDifference(childPath, expected.get(name), produced.get(name));
                }
            }
            final Iterator<String> producedNames = produced.fieldNames();
            while (producedNames.hasNext()) {
                final String name = producedNames.next();
                if (!expected.has(name)) {
                    return "at `" + (path + "/" + name) + "`: present only in the produced document";
                }
            }
        }
        if (expected.isArray() && produced.isArray()) {
            final int max = Math.max(expected.size(), produced.size());
            for (int i = 0; i < max; i++) {
                final JsonNode expectedItem = expected.path(i);
                final JsonNode producedItem = produced.path(i);
                if (!expectedItem.equals(producedItem)) {
                    return firstDifference(path + "[" + i + "]", expectedItem, producedItem);
                }
            }
        }
        return "at `" + path + "`: expected " + expected + ", produced " + produced;
    }
}
