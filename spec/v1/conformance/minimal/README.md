# `minimal`

The smallest thing that is still a contract: one key, no annotations, no external surface, no
reserved variable.

It is here to pin what a producer emits when nothing was said. Every field of the envelope is still
present — `external` is an empty declaration rather than an absent one, `values` and the four alias
arrays are empty rather than null — and a producer that omits an empty collection produces a
document every consumer has to special-case.

## What it pins

- The envelope's six fields, in order.
- `schema.loader[]` for a loader with no reserved variable: two entries, `config` and `secrets_dir`.
- A key with `#[serde(default = "…")]`: `required: false`, and an observed default of `public`
  rather than the empty string the type would give.
- `text_form: "text"` and a `constraint` of `{"type": "string"}` for a plain string key, with **no**
  `text_constraint` — any text is a well-formed string, so there is nothing for a pattern to say,
  and `text_form` is what distinguishes that from "nothing could be determined".

## Source

Reference implementation — `tests/spec.rs`:

```rust
#[derive(Deserialize, Serialize, Describe)]
struct Minimal {
    /// Bundle directory the readiness probe checks.
    #[serde(default = "default_dist")]
    dist_dir: String,
}

impl Default for Minimal {
    fn default() -> Self {
        Self { dist_dir: default_dist() }
    }
}

fn default_dist() -> String {
    "public".to_owned()
}

Terrace::new("MINIMAL_")
    .schema::<Minimal>()
    .with_defaults_from(&Minimal::default())?
    .into_contract(App::new("minimal").version("1.0.0"))
    .build()?
```

`Default` is hand-written rather than derived, and that is not boilerplate: the observed default is
whatever the producer serialises, so a derived `Default` would publish `""` for a field whose
annotation supplies `public`. An implementation in another language has the same trap wherever its
"default instance" and its "default annotation" are two different things.
