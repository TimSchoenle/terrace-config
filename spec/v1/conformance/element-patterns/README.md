# `element-patterns`

Tightenings inside a key. A handbook's chapters are each a map of locale to text, twice over, and the
runtime check holds every one of them to rules no type states: locale tags as names, text that is not
blank, an English heading. A contract that can tighten only a key's own constraint cannot say any of
that, because everything below a map sits inside its element schema.

## What it pins

| Key | Pins |
|---|---|
| `default_locale` | `pattern` on a string key — the locale tag, `LocaleTag::from_str` exactly. Version 2 vocabulary. Its default `en` matches, so it survives. |
| `chapters` | Three positions inside the element: `propertyNames` on `*.body`, `pattern` on every `*.body.*` text (the non-blank class), and `required: ["en"]` on `*.title`. The default chapter satisfies all three, so the key keeps its default. `propertyNames` puts the document at `schema_version: 3`. |

**In `rendered/`.**

- `markdown*.md` — the type cell names each tightening by its position:
  `at \`*.body\`: entry names match \`…\``, `at \`*.body.*\`: matches \`…\``,
  `at \`*.title\`: must contain: \`en\``; the string key's reads `matches \`…\``.
- `config.toml` — `# Matches: …` under the string key, and `# Entry names match at *.body: …`,
  `# Matches at *.body.*: …`, `# Must contain at *.title: en` under the map.
- `schema.json` — each keyword at its position inside the key's property schema.

**Consistency rules a producer is held to here.**

- The non-blank pattern is Unicode's `White_Space` class, negated: the negation of Rust's
  `str::trim().is_empty()`, exactly. Not `\S`, which the portable subset refuses — ECMA-262's `\s`
  holds U+FEFF.
- A position is addressed only where the type described one. Neither a single entry of a map, whose
  name the operator chooses, nor an undeclared field is a position.

## Source

Reference implementation — `tests/spec.rs`:

```rust
#[derive(Deserialize, Serialize, Describe)]
struct Handbook {
    /// The locale a chapter falls back to.
    #[serde(default = "english")]
    default_locale: String,
    /// Chapters, by slug.
    #[config(element)]
    #[serde(default = "default_chapters")]
    chapters: BTreeMap<String, Chapter>,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Chapter {
    /// Heading, by locale.
    #[serde(default)]
    title: BTreeMap<String, String>,
    /// Text, by locale.
    #[serde(default)]
    body: BTreeMap<String, String>,
}

struct HandbookRefinements;

impl Refine for HandbookRefinements {
    fn refinements(&self) -> Vec<(String, Refinement)> {
        vec![
            ("default_locale".to_owned(), Refinement::pattern(LOCALE)),
            ("chapters.*.body".to_owned(), Refinement::entry_names(LOCALE)),
            ("chapters.*.body.*".to_owned(), Refinement::non_blank()),
            ("chapters.*.title".to_owned(), Refinement::required_entries(["en"])),
        ]
    }
}

Terrace::new("BOOK_")
    .schema::<Handbook>()
    .with_defaults_from(&Handbook::default())?
    .refine_with("", &HandbookRefinements)?
    .into_contract(App::new("handbook").version("1.0.0"))
    .build()?
```

`LOCALE` is `^[A-Za-z]{2,3}(?:[-_][A-Za-z]{4})?(?:[-_](?:[A-Za-z]{2}|[0-9]{3}))?$`; `english()` is
`"en"`, and `default_chapters()` holds one `intro` chapter with an English title and body.

`terrace-config-java` — `JavaConformanceTest`, fixtures `HandbookConfig` and `Chapter` (the map
annotated `@Element`), refined through `Schema#refineWith("", …)` with the same four refinements.
Its producer publishes the same element schema for `chapters` — `additionalProperties` over the
`Chapter` object, and a map of strings under each of its fields — so every position above exists
there too. `publishesTheSameConstraintsAsTheSharedSpecCorpus` holds each key's whole `constraint` to
this document, and `publishesTheSameRefinementsAsTheSharedSpecCorpus` its `schema_version`,
`required` and which defaults survive.
