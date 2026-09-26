# `required-entries`

A map-typed key whose required entries were supplied at schema-build time rather than read off a
type: a constraint the application decides at runtime, published so a chart can see it.

The shape comes from a real failure. A legal-pages library holds its documents as a map, and the
host refuses to start unless the map holds `imprint` and `privacy`. The type says "a map"; a
contract derived from the type alone publishes `required: false` with `default_value: {}`, and a
chart rendering exactly that passes every gate and fails at boot.

## What it pins

| Key | Pins |
|---|---|
| `legal.documents` | `required` inside `constraint`, sorted, beside the element schema under `additionalProperties` — the map stays open. Its observed default `{}` lacks both entries, so the key is published `required: true` with `default` and `default_value` both `null`: a default the image itself rejects does not supply the key. |
| `legal.links` | The same refinement on a map whose default already holds the entry. The default survives, and `required` stays `false`. |
| `dist_dir` | An ordinary key beside them, unchanged. |

**In `rendered/`.**

- `markdown*.md` — the map's type cell reads `…, must contain: \`imprint\`, \`privacy\``, and its
  flags read `required`. Derived from `constraint.required`, never from a field of its own.
- `config.toml` — a `# Must contain: imprint, privacy` line under the type, and the required key
  written uncommented.
- `schema.json` — `required: ["imprint", "privacy"]` in the key's property schema, `documents` in
  the `[legal]` table's `required` list, and `legal` in the root's: a key made required by a
  refinement makes its tables required exactly as one required by its type does.

**Consistency rules a producer is held to here.**

- A refinement tightens and never loosens. What the type stated — the element schema, the open map —
  is unchanged.
- The default/required rule is applied whichever order the refinement and the defaults were
  observed in; the document does not record which, and must not differ by it.
- No `default_value` fails its own `constraint` — the ninth refusal in
  [`FORMAT.md`](../../FORMAT.md#what-a-producer-must-refuse).

## Source

Reference implementation — `tests/spec.rs`:

```rust
#[derive(Deserialize, Serialize, Describe)]
struct Site {
    /// Bundle directory the readiness probe checks.
    #[serde(default = "default_dist")]
    dist_dir: String,
    #[config(nested)]
    legal: LegalPages,
}

#[derive(Deserialize, Serialize, Describe)]
struct LegalPages {
    /// Legal documents, by the name the site links them under.
    #[config(element)]
    #[serde(default)]
    documents: BTreeMap<String, LegalDocument>,
    /// Footer links, by label.
    #[serde(default = "default_links")]
    links: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct LegalDocument {
    /// Heading of the page.
    title: String,
    /// Markdown body.
    #[serde(default)]
    body: String,
}

struct LegalPagesRefinements;

impl Refine for LegalPagesRefinements {
    fn refinements(&self) -> Vec<(String, Refinement)> {
        vec![
            ("documents".to_owned(), Refinement::required_entries(["imprint", "privacy"])),
            ("links".to_owned(), Refinement::required_entries(["home"])),
        ]
    }
}

Terrace::new("SITE_")
    .schema::<Site>()
    .with_defaults_from(&Site::default())?
    .refine_with("legal", &LegalPagesRefinements)?
    .into_contract(App::new("site").version("1.0.0"))
    .build()?
```

`default_links()` returns `{"home": "/"}`; `Site::default()` and `LegalPages::default()` are
hand-written for [`minimal`](../minimal/)'s reason. The doc comments' second paragraphs are elided
above and present in the fixture.

`terrace-config-java` — `JavaConformanceTest`, fixture `RequiredEntriesConfig`, refined through
`Schema#refineWith("legal", …)` with the same two refinements. `documents` is a
`Map<String, LegalDocument>` annotated `@Element`, so it publishes the same element schema as this
document: `publishesTheSameConstraintsAsTheSharedSpecCorpus` holds each key's whole `constraint` to
it, and `publishesTheSameRefinementsAsTheSharedSpecCorpus` the refinement's own effects —
`required`, `constraint.required`, which defaults survive — key by key.
