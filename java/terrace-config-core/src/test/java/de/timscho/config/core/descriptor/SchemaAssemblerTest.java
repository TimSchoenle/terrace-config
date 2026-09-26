package de.timscho.config.core.descriptor;

import static org.assertj.core.api.Assertions.assertThat;

import de.timscho.config.core.descriptor.KeyDescriptor.ContainerKind;
import de.timscho.config.core.model.Dialect;
import de.timscho.config.core.model.Key;
import java.util.List;
import java.util.Map;
import java.util.Set;
import org.jspecify.annotations.Nullable;
import org.junit.jupiter.api.Test;

/**
 * The element schema a container key's {@code constraint} carries, pinned against what the Rust
 * crate's {@code rust_type::interpret_with} and {@code json_schema::element_object} publish for the
 * same shapes.
 */
class SchemaAssemblerTest {

    private static Dialect dialect() {
        return Dialect.builder()
                .prefix("TEST_")
                .nestingSeparator("__")
                .indirectionSuffix("_FILE")
                .build();
    }

    private static KeyDescriptor leaf(final String name, final String typeName, final boolean hasDefault) {
        return field(name, typeName, ContainerKind.NONE, null, List.of(), false, hasDefault, List.of());
    }

    private static KeyDescriptor field(
            final String name,
            final String typeName,
            final ContainerKind container,
            @Nullable final ElementDescriptor element,
            final List<KeyDescriptor> nestedKeys,
            final boolean closed,
            final boolean hasDefault,
            final List<String> aliases) {
        return new KeyDescriptor(
                name,
                name + " docs.",
                name + " docs.",
                typeName,
                container,
                false,
                null,
                List.of(),
                null,
                nestedKeys,
                element,
                closed,
                hasDefault,
                aliases);
    }

    private static ElementDescriptor scalar(final String typeName) {
        return new ElementDescriptor(typeName, List.of(), null, List.of());
    }

    private static ElementDescriptor struct(final String typeName, final List<KeyDescriptor> keys) {
        return new ElementDescriptor(typeName, List.of(), null, keys);
    }

    private static Map<String, Object> constraintOf(final KeyDescriptor field) {
        final List<Key> keys = SchemaAssembler.assemble(
                        new TypeDescriptor("test.Config", TypeDescriptor.Kind.STRUCT, List.of(field), List.of(), false),
                        dialect(),
                        Set.of())
                .getKeys();
        assertThat(keys).hasSize(1);
        return keys.getFirst().getConstraint();
    }

    @Test
    void aSequenceOfScalarsCarriesItsItemType() {
        assertThat(constraintOf(field(
                        "hosts",
                        "List<String>",
                        ContainerKind.LIST,
                        scalar("String"),
                        List.of(),
                        false,
                        true,
                        List.of())))
                .isEqualTo(Map.of("type", "array", "items", Map.of("type", "string")));
    }

    @Test
    void aMapOfScalarsCarriesItsValueTypeWithTheElementsBound() {
        final ElementDescriptor bounded =
                new ElementDescriptor("Integer", List.of(), new RangeConstraint(1.0, null, null, null), List.of());

        assertThat(constraintOf(field(
                        "weights",
                        "Map<String,Integer>",
                        ContainerKind.MAP,
                        bounded,
                        List.of(),
                        false,
                        true,
                        List.of())))
                .isEqualTo(Map.of("type", "object", "additionalProperties", Map.of("type", "integer", "minimum", 1.0)));
    }

    /** A descriptor generated before plain-leaf elements were reported publishes what it always did. */
    @Test
    void aContainerWithNoElementDescriptorPublishesOnlyItsOwnType() {
        assertThat(constraintOf(field(
                        "links", "Map<String,String>", ContainerKind.MAP, null, List.of(), false, true, List.of())))
                .isEqualTo(Map.of("type", "object"));
        assertThat(constraintOf(
                        field("hosts", "List<String>", ContainerKind.LIST, null, List.of(), false, true, List.of())))
                .isEqualTo(Map.of("type", "array"));
    }

    @Test
    void anOpenStructElementIsItsKeysWithDescriptionsAndRequired() {
        final ElementDescriptor document =
                struct("LegalDocument", List.of(leaf("title", "String", false), leaf("body", "String", true)));

        assertThat(constraintOf(field(
                        "documents",
                        "Map<String,LegalDocument>",
                        ContainerKind.MAP,
                        document,
                        List.of(),
                        false,
                        true,
                        List.of())))
                .isEqualTo(Map.of(
                        "type",
                        "object",
                        "additionalProperties",
                        Map.of(
                                "type", "object",
                                "properties",
                                        Map.of(
                                                "title", Map.of("description", "title docs.", "type", "string"),
                                                "body", Map.of("description", "body docs.", "type", "string")),
                                "required", List.of("title"))));
    }

    /**
     * Closed exactly where the type says so: the element's own level from the container field's
     * {@code closed}, a {@code @Nested} level inside it from that field's — and the open level
     * between them stays open.
     */
    @Test
    void aClosedStructElementIsClosedOnlyWhereItsTypesAre() {
        final KeyDescriptor target = field(
                "target",
                "Target",
                ContainerKind.NONE,
                null,
                List.of(leaf("host", "String", true)),
                true,
                true,
                List.of());
        final KeyDescriptor meta = field(
                "meta",
                "Meta",
                ContainerKind.NONE,
                null,
                List.of(leaf("owner", "String", true)),
                false,
                true,
                List.of());
        final ElementDescriptor route = struct("Route", List.of(target, meta));

        final Map<String, Object> items = element(
                constraintOf(
                        field("routes", "List<Route>", ContainerKind.LIST, route, List.of(), true, true, List.of())),
                "items");

        assertThat(items).containsEntry("additionalProperties", false);
        assertThat(property(items, "target")).containsEntry("additionalProperties", false);
        assertThat(property(items, "meta")).doesNotContainKey("additionalProperties");
    }

    /** An element struct holding a container of structs nests its element schema in turn. */
    @Test
    void elementSchemasComposeAsDeepAsTheTypesStack() {
        final ElementDescriptor page = struct("Page", List.of(leaf("text", "String", false)));
        final KeyDescriptor pages =
                field("pages", "List<Page>", ContainerKind.LIST, page, List.of(), false, true, List.of());
        final KeyDescriptor locales = field(
                "locales",
                "Map<String,String>",
                ContainerKind.MAP,
                scalar("String"),
                List.of(),
                false,
                true,
                List.of());
        final ElementDescriptor chapter = struct("Chapter", List.of(pages, locales));

        final Map<String, Object> chapterSchema = element(
                constraintOf(field(
                        "chapters",
                        "Map<String,Chapter>",
                        ContainerKind.MAP,
                        chapter,
                        List.of(),
                        false,
                        true,
                        List.of())),
                "additionalProperties");

        assertThat(element(property(chapterSchema, "pages"), "items"))
                .isEqualTo(Map.of(
                        "type", "object",
                        "properties", Map.of("text", Map.of("description", "text docs.", "type", "string")),
                        "required", List.of("text")));
        assertThat(property(chapterSchema, "locales"))
                .isEqualTo(Map.of(
                        "description", "locales docs.",
                        "type", "object",
                        "additionalProperties", Map.of("type", "string")));
    }

    /** An alias inside an element is a property of its own, and a required one is a choice. */
    @Test
    void anAliasedRequiredElementFieldIsAChoiceOfSpellings() {
        final KeyDescriptor user =
                field("user", "String", ContainerKind.NONE, null, List.of(), false, false, List.of("username"));
        final Map<String, Object> items = element(
                constraintOf(field(
                        "accounts",
                        "List<Account>",
                        ContainerKind.LIST,
                        struct("Account", List.of(user)),
                        List.of(),
                        false,
                        true,
                        List.of())),
                "items");

        assertThat(property(items, "username"))
                .isEqualTo(Map.of("description", "Another spelling of `user`.", "type", "string"));
        assertThat(items).doesNotContainKey("required");
        assertThat(items)
                .containsEntry(
                        "allOf",
                        List.of(Map.of(
                                "anyOf",
                                List.of(
                                        Map.of("required", List.of("user")),
                                        Map.of("required", List.of("username"))))));
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> element(final Map<String, Object> schema, final String keyword) {
        return (Map<String, Object>) schema.get(keyword);
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> property(final Map<String, Object> schema, final String name) {
        return (Map<String, Object>) ((Map<String, Object>) schema.get("properties")).get(name);
    }
}
