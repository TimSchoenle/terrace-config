# `full-surface`

Every field of a key a producer can be asked to fill, in one document, plus the external surface and
a reserved loader variable.

The case a second implementation works through field by field. Where `minimal` pins the shape,
this one pins the *values* — and most of the disagreements between two implementations will be
here, in a `constraint` derived from a type or a `text_constraint` measured against a binder.

## What it pins

**Per key.**

| Key | Pins |
|---|---|
| `dist_dir` | A plain string with a default. No `text_constraint`. |
| `api_base` | `required: true` with `default: null` — a key some layer must supply, which is *not* a JSON Schema `required` list entry. |
| `log_level` | A choice: `values`, a document-space `enum` in `constraint`, and a `text_constraint` that permits the surrounding whitespace the environment layer trims. |
| `sample_rate` | An annotated interval narrowing what the type justified, and `text_form: "unknown"` — no measured pattern describes what a 64-bit float's parse accepts. |
| `methods` | A container of choices: `text_form: "structured"`, and a `constraint` that nests the element's shape under `items`. |
| `github.username` | An alias, and the alias's own environment spelling — a chart using it is a correct deployment, and a consumer checking only the canonical name would reject it. |
| `github.token` | A secret: `secret: true`, `default: null`, and `writeOnly` in the `json_schema` half. |
| `github.ttl_secs` | A `note` qualifying a default that reads wrong without one — `0` meaning permanent. |

**Per document.**

- A reserved loader variable, `PORTFOLIO_PROFILE`, in `schema.loader[]` with `role: "reserved"` and
  in no key.
- `#[serde(deny_unknown_fields)]` on the root, so the `json_schema` half closes the object.
- A declared external variable with a type and a default, one with neither, an ignore pattern with a
  trailing wildcard, one without, and `unknown: "reject"`.

## Deliberately absent

- A key with `unreachable` — that is [`unnameable-key`](../unnameable-key/).
- A map-typed key. `constraint` nests under `additionalProperties` there rather than `items`, and
  the case for it should be added when a second implementation has an opinion about how its own map
  types render.

## Source

Reference implementation — `tests/spec.rs`:

```rust
#[derive(Deserialize, Serialize, Describe)]
#[serde(deny_unknown_fields)]
struct Full {
    /// Bundle directory the readiness probe checks.
    #[serde(default = "default_dist")]
    dist_dir: String,
    /// Base URL every outbound request is resolved against.
    api_base: String,
    /// How much the service says.
    #[config(values)]
    #[serde(default)]
    log_level: LogLevel,
    /// Fraction of requests sampled for tracing.
    #[config(range(min = 0.0, max = 1.0))]
    #[serde(default)]
    sample_rate: f64,
    /// Methods every route admits.
    #[config(element_values)]
    #[serde(default)]
    methods: Vec<Method>,
    #[config(nested)]
    github: Github,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Github {
    /// User whose repositories are listed.
    #[serde(alias = "user")]
    username: String,
    /// Bearer token lifting the API rate limit.
    ///
    /// Supply it as a mounted file, so the value never reaches the process environment.
    #[config(secret)]
    token: Option<String>,
    /// Revalidation interval in seconds.
    #[config(note = "permanent")]
    #[serde(default)]
    ttl_secs: u64,
}

Terrace::new("PORTFOLIO_")
    .reserve("PORTFOLIO_PROFILE")
    .schema::<Full>()
    .with_defaults_from(&Full::default())?
    .into_contract(App::new("portfolio").version("2.5.0"))
    .external(
        External::new()
            .var(ExternalVar::new("PORT").owner("runtime").ty("u16").default("8080")
                 .docs("Bind port. Read by the server toolchain, not by this loader."))
            .var(ExternalVar::new("RUST_LOG").owner("tracing").ty("String"))
            .ignore("KUBERNETES_*")
            .ignore("HOSTNAME")
            .unknown(Unknown::Reject),
    )
    .build()?
```

## Reproducing it elsewhere

The types are Rust's, and two of them have no direct equivalent in another language. Translate the
*meaning*, not the spelling:

- `u64` for `ttl_secs` is an integer with a lower bound of zero and no expressible upper bound. A
  Java implementation over `long` publishes `{"type": "integer"}` with whatever bound it can
  certainly justify, and that is a legitimate divergence — see tier 3 in
  [`CONFORMANCE.md`](../../CONFORMANCE.md).
- `Option<String>` for `token` is "may be absent", which is what `required: false` carries. The type
  name in `ty` is the producer's own and is never read by a consumer.
