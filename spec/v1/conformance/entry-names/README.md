# `entry-names`

A map-typed key whose entry *names* are held to a pattern supplied at schema-build time, beside the
required entries [`required-entries`](../required-entries/) pins. The pattern is `propertyNames`
inside `constraint`, so the document is published at `schema_version: 3`.

The shape comes from the same legal-pages library. It refuses a document whose name is not a slug —
one to 64 ASCII bytes, the first a lower-case letter or digit, the rest those or `_` and `-` — and a
contract that says nothing about names lets a deployment tool pass `{"Terms": …}` to an image that
refuses it at boot.

## What it pins

| Key | Pins |
|---|---|
| `legal.documents` | `propertyNames: {"pattern": "^[a-z0-9][a-z0-9_-]{0,63}$"}` inside `constraint`, beside `required` and the element schema under `additionalProperties`. The required entries are slugs, which a producer checks before publishing either refinement. Its default `{}` lacks both entries, so the key is published `required: true` with no default, exactly as in `required-entries`. |
| `legal.links` | `propertyNames: {"pattern": "^[a-z]+$"}` and a required `home`. The default `{"home": "/"}` satisfies both, so it survives and `required` stays `false`. |
| `schema.schema_version` | `3`: the lowest version whose vocabulary holds every keyword the document carries. A document without `propertyNames` stays at `2`. |

**In `rendered/`.**

- `markdown*.md` — the map's type cell reads `…, must contain: …, entry names match \`…\``, both
  derived from `constraint`.
- `config.toml` — an `# Entry names match: …` line under `# Must contain: …`.
- `schema.json` — `propertyNames` in the key's property schema, beside `required`.

**Consistency rules a producer is held to here.**

- The pattern is inside the portable subset —
  [`FORMAT.md`, *Portable patterns*](../../FORMAT.md#portable-patterns).
- Every required entry matches the pattern, in whichever order the two refinements were applied.
- The default/required rule is applied to a default the pattern rejects as to one lacking an entry,
  whichever order the refinement and the defaults were observed in.

## Source

Reference implementation — `tests/spec.rs`, reusing `Site`, `LegalPages` and `LegalDocument` from
[`required-entries`](../required-entries/):

```rust
struct NamedLegalPagesRefinements;

impl Refine for NamedLegalPagesRefinements {
    fn refinements(&self) -> Vec<(String, Refinement)> {
        let mut refinements = LegalPagesRefinements.refinements();
        refinements.push((
            "documents".to_owned(),
            Refinement::entry_names("^[a-z0-9][a-z0-9_-]{0,63}$"),
        ));
        refinements.push(("links".to_owned(), Refinement::entry_names("^[a-z]+$")));
        refinements
    }
}

Terrace::new("SITE_")
    .schema::<Site>()
    .with_defaults_from(&Site::default())?
    .refine_with("legal", &NamedLegalPagesRefinements)?
    .into_contract(App::new("site").version("1.0.0"))
    .build()?
```

`terrace-config-java` — `JavaConformanceTest`, fixture `RequiredEntriesConfig`, refined through
`Schema#refineWith("legal", …)` with the same four refinements.
`publishesTheSameRefinementsAsTheSharedSpecCorpus` holds `schema_version`, `required`,
`constraint.required`, `constraint.propertyNames` and which defaults survive to this document key by
key, and `publishesTheSameConstraintsAsTheSharedSpecCorpus` each key's whole `constraint`.
