# `optional-sections`

Sections the loader fills in when a document leaves them out, and the keys inside them that no
layer has to supply.

`required` is decided per field, and a type does not know where it is used. `Bucket` requires both
of its fields, which is true of a bucket someone writes. It is not true of `global`, which a
`default` function supplies whole when the document has no `global` table, nor of `auth`, which an
`Option` leaves unset. A producer that publishes `global.per_minute` as required sends every
consumer checking `required` after a key the image never asks for, and drops the observed default
that would have told an operator what they get instead.

## What it pins

| Key | Pins |
|---|---|
| `global.per_minute`, `global.burst` | `required: false` with the observed defaults `300` and `60`: the section's `default` supplies both, so a key inside a defaulted section is not required whatever its own field says. |
| `auth.per_minute`, `auth.burst` | `required: false` and unset: an `Option` section may be absent whole. |
| `store.url` | `required: true` with no default: a section the document has to carry keeps its own required keys. |

**The rule.** A key is required only when its own field is and every section above it is too. A
section is optional when the loader accepts a document leaving it out: it has a default of its own,
the type holding it supplies defaults for its fields, or it is optional in the language's own
sense. A flattened section has no table of its own and opens no scope: its keys are the parent's.

**In `rendered/`.** `schema.json` carries no `required` entry for `global` or `auth` and one for
`store`, so a values file that leaves out either optional section validates.

## Source

Reference implementation — `tests/spec.rs`:

```rust
#[derive(Deserialize, Serialize, Describe)]
struct Limiter {
    /// Applies to every route without a stricter class.
    #[config(nested)]
    #[serde(default = "Bucket::generous")]
    global: Bucket,
    /// Credential-handling routes. Unlimited when unset.
    #[config(nested)]
    auth: Option<Bucket>,
    /// Where the shared counters live.
    #[config(nested)]
    store: Store,
}

#[derive(Deserialize, Serialize, Describe)]
struct Bucket {
    /// Requests allowed per minute, which is the bucket's refill rate.
    per_minute: u32,
    /// The most requests a client may spend back-to-back.
    burst: u32,
}

#[derive(Deserialize, Serialize, Describe)]
struct Store {
    /// Connection URL of the counter store.
    url: String,
}

Terrace::new("LIMITER_")
    .schema::<Limiter>()
    .with_defaults_from(&Limiter::default())?
    .into_contract(App::new("limiter").version("1.0.0"))
    .build()?
```

`Limiter::default()` holds `Bucket::generous()` for `global`, `None` for `auth` and an empty `url`
for `store`. The last is not published: a required key keeps no default.
