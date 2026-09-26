# `conditions`

Conditions between the fields of every element of a map. A legal-pages library's documents are each
hosted — a body — or external — a URL — never both and never neither; and a document asking for
consent needs a version, a hosted body, and an effective date once it grants a grace period. No type
states any of that, and each is a relation between fields, not a property of one.

## What it pins

| Key | Pins |
|---|---|
| `documents` | Two members of the element's `allOf`, each carrying `description`. The first is `oneOf` over `url` being set and `body` being non-empty — exactly one holds. The second is `if` the consent requirement is set and not `none`, `then` all of: no `url`, a non-blank `consent.version`, and — nested in that consequence — `if` `consent.grace_days` is above 0 `then` `consent.effective` is set. Both defaults, one hosted with a consent rule and one external, satisfy both, so the key keeps its default. The document is at `schema_version: 4`. |

The grace rule sits inside the consent rule's consequence on purpose. The runtime returns before any
other consent check when the requirement is `none`, so a grace rule stated on its own would refuse
`{"requirement": "none", "grace_days": 5}`, which the runtime accepts.

**In `rendered/`.**

- `markdown*.md` — the type cell names each condition by its position and its sentence:
  ``at `*`: exactly one of: [`url` is set; `body` is not empty]``.
- `config.toml` — one `# Holds at *: …` line per condition.
- `schema.json` — both members under the element's `allOf`, each with its `description`.

**Consistency rules a producer is held to here.**

- A condition names only fields the struct declares, through nested structs, and asks of each only
  what its type answers: emptiness of a map, sequence or string, a bound of a number, a pattern of a
  string, a value the field's own constraint admits.
- An unset field — an `Option` holding `None` — is absent from a published default, never `null`:
  what the loader sees for it is absence, and `required` is how a condition asks for presence.

## Source

Reference implementation — `tests/spec.rs`:

```rust
#[derive(Deserialize, Serialize, Describe)]
struct Policies {
    /// Documents, by slug.
    #[config(element)]
    #[serde(default = "default_documents")]
    documents: BTreeMap<String, Policy>,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Policy {
    /// Text, by locale. Empty for an external document.
    #[serde(default)]
    body: BTreeMap<String, String>,
    /// Where an external document lives.
    #[serde(default)]
    url: Option<String>,
    #[config(nested)]
    #[serde(default)]
    consent: PolicyConsent,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct PolicyConsent {
    /// What a visitor is asked for.
    #[config(values)]
    #[serde(default)]
    requirement: ConsentRequirement, // `none` or `accept`
    /// The version a visitor consents to.
    #[serde(default)]
    version: Option<String>,
    /// Days a changed document may still be shown without renewed consent.
    #[serde(default)]
    grace_days: u16,
    /// When the current version took effect.
    #[serde(default)]
    effective: Option<String>,
}

// Relative to the root:
("documents.*", Refinement::holds(Condition::exactly_one([
    Condition::present("url"),
    Condition::non_empty("body"),
]))),
("documents.*", Refinement::holds(Condition::when(
    Condition::not_equals("consent.requirement", "none"),
    Condition::all([
        Condition::absent("url"),
        Condition::matches("consent.version", Refinement::NON_BLANK),
        Condition::when(
            Condition::above("consent.grace_days", 0),
            Condition::present("consent.effective"),
        ),
    ]),
))),
```

`default_documents()` holds a hosted `privacy` document asking for consent at version `1` with a
14-day grace period from `2026-09-22`, and an external `terms` document.

`terrace-config-java` states the same conditions with the same encodings and sentences —
`ConditionTest` pins them on a hand-built constraint — but does not produce this case, for the
reason [`element-patterns`](../element-patterns/) gives: its producer publishes no element schema for
a map yet. The codec modules round-trip this document.
