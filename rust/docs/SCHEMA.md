# Generating the configuration reference

The `schema` feature derives a reference table, an example file and a JSON Schema from the types.

The git URL and the tag to pin are in the [README](../README.md#installation).

The loader never learns the shape of a config — it hands the merged figment to `serde` and takes
back a `T`. The `schema` feature inverts that, so the reference table every service needs is
generated from the type instead of maintained beside it.

```toml
terrace-config = { git = "…", tag = "…", features = ["schema"] }
```

Add one derive to the structs you already have. Everything else is read from the `#[serde(...)]`
attributes that are there anyway, so nothing is annotated twice:

```rust
use serde::{Deserialize, Serialize};
use terrace_config::schema::Describe;

#[derive(Deserialize, Serialize, Default, Describe)]
struct Github {
    /// User whose repositories `update-repos` lists.
    #[serde(alias = "user")]
    username: String,
    /// Bearer token lifting the GitHub API rate limit.
    #[config(secret)]
    token: Option<String>,
    /// Revalidation interval in seconds.
    #[config(note = "permanent")]
    #[serde(default)]
    ttl_secs: u64,
}
```

| Attribute | Effect |
|-----------|--------|
| `#[config(nested)]` | Recurse into the field's type instead of treating it as a leaf |
| `#[config(secret)]` | Render the default as `<redacted>`, and mark the key |
| `#[config(values)]` | Report the field type's variants as the values the key accepts |
| `#[config(values_from = "…")]` | Report another type's variants as the values the key accepts |
| `#[config(values("…", "…"))]` | Report a literal list as the values the key accepts |
| `#[config(range(…))]` | Bound the number the key accepts: `min`, `max`, `exclusive_min`, `exclusive_max` |
| `#[config(element)]` | Report the shape of one element of a container-typed key |
| `#[config(element_values)]` | Report the values one element of a container-typed key accepts |
| `#[config(element_values_from = "…")]` | Another type's variants, one level down |
| `#[config(element_values("…", "…"))]` | The same literal list, one level down |
| `#[config(note = "…")]` | Annotate the observed default with prose |
| `#[config(skip)]` | Omit the key without affecting deserialisation |

`#[serde(deny_unknown_fields)]` is read too, and is the one thing on this page that is not a
`#[config(...)]` attribute — see [Closed structs](#closed-structs).

The attributes that say what a key *holds* are opt-in but not optional: a field whose type is a
bare name this crate does not recognise is a compile error rather than a key with no shape — see
[A named type has to say something](#a-named-type-has-to-say-something).

Three things are why this is a derive rather than runtime reflection. The key path, the
environment spelling and whether a value is required are all recoverable at runtime; the sentence
saying what a key is *for*, the type it takes, and the variants an enum-valued key accepts are
gone before any runtime sees the type.

That last one is what `Describe` on an **enum** is for. A struct of named fields *has*
configuration keys; an enum of unit variants *is* the set of values one key accepts, so the derive
reports those spellings instead — `#[serde(rename_all)]` applied, because a table printing `Info`
where the file must say `info` documents a value nobody can set:

```rust
#[derive(Deserialize, Serialize, Default, Describe)]
#[serde(rename_all = "lowercase")]
enum LogLevel { Trace, Debug, #[default] Info, Warn }

#[derive(Deserialize, Serialize, Default, Describe)]
struct Observability {
    /// How much the service says.
    #[config(values)]
    #[serde(default)]
    log_level: LogLevel,
}
```

## A named type has to say something

`values` and `nested` have to stay opt-in. A derive has only tokens, so it cannot tell whether a
named type implements `Values` or `Describe`, and guessing is unsound in the direction that
matters: a type whose `Deserialize` is something else — `#[serde(try_from = "String")]` over a
case-insensitive `FromStr` — accepts spellings that are not in the variant list, so publishing that
list unasked would emit a schema rejecting a file the loader takes.

Opt-in is not the same as optional. A field whose type is a bare name this crate does not
recognise — not one of the leaf spellings, not a container — publishes **nothing**: no type, no
values, no keys, and a row indistinguishable from one that was described. That is the gap this
whole feature exists to close, so it is a compile error:

```rust
#[derive(Deserialize, Serialize, Default, Describe)]
struct Observability {
    /// How much the service says.
    #[serde(default)]
    log_level: LogLevel,   // error: publishes no shape at all
}
```

Six attributes resolve it, and the error names the field, its type and all of them:

| Attribute | When it is the answer |
|-----------|-----------------------|
| `#[config(values)]` | The type implements `Values` — it derives `Describe` as an enum |
| `#[config(values_from = "…")]` | It does not, and another type does |
| `#[config(values("…", "…"))]` | Neither does, and the list says what it accepts |
| `#[config(nested)]` | The type implements `Describe`, and its keys belong under this one |
| `#[config(element)]` / `#[config(element_values)]` | The field is a container, and the *element* is one of those |
| `#[config(range(…))]` | It is a number in an interval, and the interval is the whole of what a schema can say |
| `#[config(skip)]` | The field genuinely has nothing to publish |

Two things worth knowing:

- **It fires on a bare name only.** A container's *element* is checked the same way one level
  down, so `Vec<LogLevel>` is refused and `Vec<String>` is not. A tuple, a qualified path and a
  generic type this crate does not read as a container are left alone: none of them is a shape the
  six attributes have an answer for, so an error naming them would be a dead end rather than a fix.
- **`skip` omits the key.** It is the escape hatch for a field with no publishable shape, not a way
  to silence the error while keeping the row — a domain newtype over `String` that has to keep its
  key needs `range`, a description, or a hand-written `Describe`.

## Values a trait cannot reach

`#[config(values)]` reads the field type's own `Values` implementation, and the orphan rule puts a
foreign enum out of reach: an application can implement neither `Values` nor `Describe` for
`other::Compression`, and this crate will not depend on `other` to do it here. Two attributes reach
past the trait, and they are not equals.

**`values_from` names a type that does implement it.** A `#[serde(remote = "…")]` mirror is the
case it exists for: serde matches the mirror's variants exactly, `Describe` on the mirror reports
those same variants under the same `rename_all`, and the two cannot drift because they are one
declaration. Nothing is spelled twice, and the compiler still checks it:

```rust
#[derive(Deserialize, Describe)]
#[serde(remote = "other::Compression", rename_all = "lowercase")]
enum CompressionDef { Gzip, Zstd, None }

#[derive(Deserialize, Serialize, Default, Describe)]
struct Wire {
    /// How payloads are compressed.
    #[serde(with = "CompressionDef", default)]
    #[config(values_from = "CompressionDef")]
    compression: other::Compression,
}
```

**`values("…", "…")` lists the spellings**, for a type no mirror can be written for — serde's
`remote` needs a shape it can mirror, and a newtype over a private enum has none:

```rust
#[derive(Deserialize, Serialize, Default, Describe)]
struct Wire {
    /// How payloads are compressed.
    #[serde(deserialize_with = "compression", default)]
    #[config(values("gzip", "zstd", "none"))]
    compression: other::Compression,
}
```

Both have element forms — `element_values_from = "…"` and `element_values("…", "…")` — for a
container of one. What comes out is what the derived form produces: a bare `enum` in `constraint`,
the trimming `pattern` in `text_constraint`, and the same `` `gzip` \| `zstd` `` cell in the table,
so no rendering and no consumer can tell the routes apart.

Three things worth knowing before reaching for a list:

- **It is an assertion this crate cannot check.** Nothing in a derive reads a foreign type's
  `Deserialize`, so a list that disagrees with it publishes a schema rejecting a file the loader
  takes. It is the author's to keep true, which is the standing `#[config(note = "…")]` already
  has. Prefer `values_from` wherever a mirror can be written: variants read off a type cannot drift
  from it.
- **All four satisfy the diagnostic above**, which is the other half of why they exist. Before
  them, a foreign enum had no honest annotation at all.
- **Bare `#[config(values)]` is unchanged.** It still means "use the `Values` implementation", and
  an empty list or a repeated spelling is rejected rather than published.

## The accepted set is the deserialiser's answer, not the type's

Two shapes are refused outright, because in both the derived variants provably are not the wire
form:

- an **enum** carrying `#[serde(try_from = "…")]` or `#[serde(from = "…")]` cannot derive `Values`
  at all — its `Deserialize` reads a different type and converts;
- a **field** carrying `#[serde(with = "…")]` or `#[serde(deserialize_with = "…")]` cannot take a
  bare `values` or `element_values` — the same, one level down.

`#[serde(remote = "…")]` is deliberately not on that list: a mirror's variants are matched exactly,
which is what makes it the answer rather than the problem.

`tracing::Level` is the worked example of why, and the one to **not** copy. It has no `Deserialize`
of its own, so a field holding one goes through its
[`FromStr`](https://docs.rs/tracing-core/0.1.36/src/tracing_core/metadata.rs.html#561) — which
matches case-insensitively **and** accepts `"1"` through `"5"`. A field annotated
`values("trace", "debug", "info", "warn", "error")` therefore publishes a schema refusing `INFO`
and `3`, both of which load. It is also a newtype over a private enum, so serde's `remote` cannot
mirror it and `values_from` has nothing to point at.

Read the accepted set off the conversion rather than off the variant names. Where it is not a fixed
set of spellings at all — as here — `#[config(skip)]` the key, implement `Describe` by hand to
publish the constraint that is true, or hold a local enum in the config struct and convert after
loading, which is the one route where every layer agrees by construction.

## A key that holds many of something

`routes: Vec<RouteConfig>` is **one** key. An array index is not a key segment and no environment
variable names one, so `routes` is a single row with a single environment spelling — and the file
it validates still has to say what one route looks like.

The type token carries half of that. `Vec<RouteConfig>` is an array, which the JSON Schema
rendering already emitted; `RouteConfig` is a name, and this crate has no type graph to look a name
up in. `#[config(element)]` supplies the other half from the type that does know:

```rust
#[derive(Deserialize, Serialize, Default, Describe)]
struct Config {
    /// Routes declared in the file.
    #[config(element)]
    #[serde(default)]
    routes: Vec<RouteConfig>,

    /// Methods each path forwards.
    #[config(element_values)]
    #[serde(default)]
    paths: HashMap<String, HashSet<Method>>,
}
```

`element` is for an element that derives `Describe` — a struct with keys of its own, nested
`#[config(nested)]` tables and all. `element_values` is for one that derives `Describe` as an
**enum**, which is the same distinction `nested` and `values` draw one level up.

What comes out is a nested schema on the key, not new keys:

```json
"routes": { "type": "array", "items": { "type": "object", "properties": { … } } },
"paths":  { "type": "object",
            "additionalProperties": { "type": "array", "uniqueItems": true,
                                      "items": { "type": "string", "enum": ["GET", "POST"] } } }
```

The containers are still read from the tokens, so they stack: the `paths` example reaches `Method`
through a map *and* a set without either being a special case. The element type is found through
`Option`, `Box`, `Arc`, `Rc` and `Cow`, into the item of a `Vec`, `VecDeque`, `HashSet`,
`BTreeSet`, `[T]` or `[T; N]`, and into the *value* of a `HashMap` or `BTreeMap` — a map's key type
is skipped, because a TOML table's keys are strings whatever the map is keyed by.

Three things worth knowing before reaching for it:

- **It is opt-in, and a container of leaves does not need it.** `Vec<String>` and
  `BTreeMap<String, u16>` are read to the bottom from the tokens and publish exactly the bytes they
  always did. A container whose element is a *named* type this crate cannot read is the case the
  [diagnostic](#a-named-type-has-to-say-something) refuses: describe the element, list its values,
  or `#[config(skip)]` the key.
- **The element type has to be spelled out.** A derive has only tokens, so
  `type Routes = Vec<RouteConfig>` is a bare identifier here and is rejected with an error saying
  so rather than guessed at. Spell the container out, or implement `Describe` by hand and call
  `Sink::repeated`.
- **`element_values` reports what the derive reports, which is serde's default wire form.** A type
  whose `Deserialize` is something else — `#[serde(try_from = "String")]` over a case-insensitive
  `FromStr` is the one that turns up — accepts spellings that are not in the variant list, and a
  schema listing only the variants would reject a file the loader takes. Do not describe such an
  element: `#[config(skip)]` the key, or list the spellings the `Deserialize` actually accepts with
  `element_values("…", "…")`. A schema that refuses a file the loader takes is the one thing this
  crate will not publish.

The environment layer is untouched by any of this. A container is still supplied as one TOML
literal, so `text_form` stays `structured` and `text_constraint` stays the bracket pattern — the
element lives in document space only.

## A number the type is too wide for

`sample_rate: f32` publishes `{"type": "number"}` and nothing more. That the value is a *fraction*
is not in the type — `f32` admits every finite float and the service takes four hundredths of
them — so every consumer that knew it has been writing `minimum: 0` and `maximum: 1` by hand.
`#[config(range(...))]` says it once, where the field is:

```rust
#[derive(Deserialize, Serialize, Default, Describe)]
struct Tracing {
    /// Share of requests that are traced.
    #[config(range(min = 0.0, max = 1.0))]
    #[serde(default)]
    sample_rate: f32,

    /// Workers, which must leave one core for the reactor.
    #[config(range(min = 1, max = 63))]
    #[serde(default)]
    workers: u16,

    /// Backoff multipliers, none of which may shrink the wait.
    #[config(range(exclusive_min = 1.0))]
    #[serde(default)]
    backoff: Vec<f64>,
}
```

It takes `min`, `max`, `exclusive_min` and `exclusive_max` — at least one, at most one per end —
and emits `minimum`, `maximum`, `exclusiveMinimum` and `exclusiveMaximum`. It works on every
numeric type the derive already reads: the integers, the `NonZero` family, `f32` and `f64`, and a
domain newtype whose spelling this crate does not recognise at all.

```json
"sample_rate": { "type": "number",  "minimum": 0.0, "maximum": 1.0 },
"workers":     { "type": "integer", "minimum": 1,   "maximum": 63 },
"backoff":     { "type": "array", "items": { "type": "number", "exclusiveMinimum": 1.0 } }
```

Three things worth knowing before reaching for it:

- **The bound lands where the type's own reading stops**, which for a container is the element:
  `Vec<f64>` bounds the numbers in the vector, because a `minimum` on the vector itself would mean
  nothing. That is the position `element` fills for a container of structs, reached through the
  same stack of `Option`, `Vec`, `HashMap` and the rest — which is why the two do not combine on
  one field.
- **A bound never widens one the type already justifies.** `max = 100_000` on a `u16` is dropped
  rather than published: `maximum` is one keyword, so it would *replace* the exact `65535` and the
  schema would then accept a file the loader refuses. A bound on a type this crate reads as a
  string or a boolean is dropped for the same reason — there is nothing there for it to mean.
- **An integer literal stays an integer.** `min = 1` publishes `1` and `min = 1.0` publishes `1.0`,
  because a consumer that walks the keywords itself reads those as different numbers, and nothing
  about the annotation asked for the other one.

The environment layer is untouched here too. A range applies to the parsed value; a `pattern`
cannot express one, and half-expressing it — digits capped at five characters for a `u16` — would
reject `00080`, which the loader takes. `text_constraint` keeps the sign-and-digits pattern it had.

## Closed structs

A struct published through `#[config(element)]` reports its `properties` and its `required`, which
leaves a consumer unable to tell it from an open map: a misspelt field passes validation. Serde
already has the answer, so it is read rather than annotated a second time:

```rust
#[derive(Deserialize, Serialize, Default, Describe)]
#[serde(deny_unknown_fields)]
struct RouteConfig {
    /// Where the route delivers.
    upstream: String,
}
```

```json
"routes": { "type": "array", "items": {
    "type": "object",
    "properties": { "upstream": { "type": "string", "description": "Where the route delivers." } },
    "required": ["upstream"],
    "additionalProperties": false } }
```

Reading it off `#[serde(deny_unknown_fields)]` is what keeps the schema and the deserialiser from
coming to disagree about which fields exist. Nothing is emitted for a struct without it — serde
accepts an undeclared field by default, and a schema refusing one would refuse a file that loads.

It closes the level the attribute is on and no other. A struct inside a closed one that did not say
so stays open, and the map *around* a closed element stays open too, because the keys in it are the
ones an operator chose.

This is a fact about the type, so it lands in `constraint` where no rendering option reaches it.
Whether an undeclared key is an error anywhere *else* in the document remains
`JsonSchema::closed`'s question — a chart's values carry keys belonging to the chart, and no
configuration type has heard of those.

## Two outputs

```rust
let schema = Terrace::new("PORTFOLIO_")
    .reserve("PORTFOLIO_PROFILE")
    .schema::<Config>()
    .with_defaults_from(&Config::default())?;

std::fs::write("docs/config.json", schema.to_json()?)?;   // the contract
std::fs::write("docs/config.md", schema.to_markdown())?;  // ready to paste
```

`to_json` is the machine-readable contract: a versioned document carrying every field of every
key, including the ones the Markdown renderer leaves out to stay readable. Point a documentation
pipeline at it and render whatever that pipeline wants.

`to_markdown` is for when the next step is `>> README.md`. It emits GitHub-flavoured tables —
one for the variables the loader itself reads, one for the keys:

| TOML | Type | Environment | Default | Flags | Purpose |
|---|---|---|---|---|---|
| `github.username` | `String` | `PORTFOLIO_GITHUB__USERNAME` | — | required | User whose repositories `update-repos` lists. |
| `github.token` | `String` | `PORTFOLIO_GITHUB__TOKEN` | unset | secret | Bearer token lifting the GitHub API rate limit. |
| `github.ttl_secs` | `u64` | `PORTFOLIO_GITHUB__TTL_SECS` | `0` (permanent) | — | Revalidation interval in seconds. |
| `log_level` | `LogLevel`: `trace` \| `debug` \| `info` \| `warn` | `PORTFOLIO_LOG_LEVEL` | `info` | — | How much the service says. |

The `Type` column is in the default set because without it a required key shows an em dash for its
default and the reader has no way to tell whether to supply a string, a number or a list. Neither
file spelling is, because both are mechanical: `Column::EnvFile` is the `Environment` cell plus the
dialect's documented suffix (`_FILE`), and `Column::SecretsFile` is the `TOML` cell with the
separator substituted. One sentence of prose covers both, where two columns push the table past
the width of a page.

The `Purpose` column carries the **summary** of the `///` comment — its first paragraph, on
rustdoc's own convention — rather than the whole of it. Write each field's documentation for
whoever reads the type; the paragraphs below the summary stay in `to_json`'s `docs` field, out of
the table, and no extra annotation is needed to keep the two in step.

`to_markdown_with` takes a `&[Column]` when those are not the columns you want — either file
spelling, or `Column::Aliases`, which is out of the default set because it is empty for almost
every key. A `#[serde(alias = "user")]` on `github.username` reports `github.user` as a full key
path, so its environment and file spellings derive exactly as the canonical one's do.

The two tables are also reachable separately, for a page that does not want them welded together:

```rust
let loader = schema.to_markdown_loader();                  // the variables, once
let keys = schema.subset("csp").to_markdown_keys(Column::DEFAULT);  // one subsystem, no preamble
```

A README with one key table per subsystem wants the loader variables above the first of them, not
repeated over every table. `to_markdown_loader` renders an empty string rather than a bare header
when a schema has no loader variables; `to_markdown_keys` always renders its header, because an
empty configuration section is a real shape and the header is what says it was generated rather
than forgotten.

Every rendering ends with a newline, so a template pipeline that appends another section needs no
separator of its own.

## A whole crate, a whole workspace, or one subsystem

`#[config(nested)]` is a trait bound, so it follows the *type*, not the file. A configuration
split across modules — or across workspace members, each deriving `Describe` beside the code that
consumes it — is walked in full by describing the root type, with nothing registered anywhere
central and no build script scanning sources. The generator lives in the binary crate that owns
the root; the members only derive.

For the other direction, `Schema::subset` slices one subsystem out for a page of its own, keeping
the real key paths:

```rust
let csp = Terrace::new("PORTFOLIO_").schema::<Config>().subset("csp");
```

`Terrace::schema_at::<Csp>("csp")` does the same from the subsystem's own type, which matters
because `schema::<Csp>()` alone would produce `cloudflare.turnstile` — a path that appears in no
configuration file anywhere.

Some workspaces have no single root at all — one binary reads `assets`, `csp` and `isr`, another
reads `github`, and keeping the two apart is the point of the split. `Schema::merge` unions the
schemas of the roots those binaries actually load, so one document covers the workspace without an
aggregate struct that exists only for the generator and can drift from every root it stands in for:

```rust
let terrace = Terrace::new("PORTFOLIO_").reserve("PORTFOLIO_PROFILE");
let everything = terrace
    .schema::<server::Config>()
    .with_defaults_from(&server::Config::default())?
    .merge(
        terrace
            .schema::<updater::Config>()
            .with_defaults_from(&updater::Config::default())?,
    );
```

Keys keep declaration order within each half, and a key both roots describe identically — the
shared key two binaries genuinely both read — is kept once. Anything else is refused: two different
descriptions of one path, two different dialects, or two different schema versions all panic, on
the same reasoning as the duplicate-path check inside `describe`. A table that quietly picks one of
two descriptions is worse than one that refuses to be generated.

## Wiring it into your own crate

Nothing here reads the environment, so a documentation job produces the same answer on a runner
where none of the variables it describes are set. `examples/config-schema.rs` in this repository
is the whole pattern; what follows is that pattern as it looks in *your* project.

**1. Take the feature.** The derive is used on your config structs, so it is a normal dependency,
not a dev-dependency. `schema-cli` adds the generator program on top of `schema`, and costs no
dependency `schema` did not already pull:

```toml
[dependencies]
terrace-config = { git = "…", tag = "…", features = ["schema-cli"] }

# The generator, so `cargo clippy --all-targets` keeps it compiling.
[[example]]
name = "config-schema"
```

**2. Add the generator**, `examples/config-schema.rs`, next to the root config type. Everything in
it is your service's own — the root type and the prefix, the app identity, the JSON Schema's
`title` and `$id`, and the external surface no derive can see. The `--format` vocabulary, the
argument parsing, the dispatch across the six renderings, the printing and the exit code are
`schema::cli::Cli`:

```rust
use std::process::ExitCode;
use myservice::Config;              // the root; nested types need only `Describe`
use terrace_config::Terrace;
use terrace_config::schema::cli::Cli;
use terrace_config::schema::{App, Docs, JsonSchema, TomlExample};

fn main() -> ExitCode {
    let schema = Terrace::new("MYSERVICE_")
        .reserve("MYSERVICE_PROFILE")
        .schema::<Config>()
        .with_defaults_from(&Config::default())
        .expect("the default config serialises");

    Cli::new(
        // `v2.5.0`, not `2.5.0`: the field exists to be compared against an image tag.
        App::new("myservice")
            .version(concat!("v", env!("CARGO_PKG_VERSION")))
            .source("https://github.com/you/myservice"),
    )
    .json_schema(
        JsonSchema::new()
            .title("myservice configuration")
            .id("https://github.com/you/myservice/config.schema.json"),
    )
    // Optional. The default suits a file kept beside a README; `Docs::Full` suits one that is the
    // only documentation an operator gets.
    .toml_example(TomlExample::new().docs(Docs::Full))
    .main(schema)
}
```

`Cli::main` is the convenient layer and also the one that decides for you: it reads
`std::env::args`, prints to stdout and returns an `ExitCode`. A service that already parses
arguments with `clap` builds a `Request` itself and calls `Cli::render`, which decides none of
that; a service that wants only the `--format` spellings takes `Format` and nothing else.

Drop to `Request` when the generator has a flag of its own — a `--scope` picking which of two
schemas to describe, a `--service` picking which binary's — because `Request::parse` refuses an
argument it does not know, and it is right to. Build one instead:

```rust
let request = Request::new(Format::Contract)
    .with_version(tag)
    .with_revision(sha)
    .with_created(timestamp);

let rendered = cli.render(&request, schema_for(scope)?)?;
```

`Config::default()` is what supplies the values in the `Default` column, so `Config` needs
`Serialize` as well as `Deserialize`. Pass whatever represents "nothing was supplied" — if your
`Default` and your `#[serde(default = "…")]` functions disagree, pass what serde would produce.

**A field holding a secret needs one more attribute.** `secrecy::SecretString` refuses to
implement `Serialize` on purpose, so a config that holds one cannot derive `Serialize` either —
which is the crate's own audience, since a config holding secrets is the reason to reach for
`terrace-config` over bare figment. The compiler says so in terms of `SerializableSecret` and
`SerializeStruct::serialize_field`, and mentions nothing about schemas:

```rust
#[derive(Deserialize, Serialize, Default, Describe)]
struct Github {
    /// Bearer token lifting the GitHub API rate limit.
    #[config(secret)]
    #[serde(skip_serializing)]
    token: Option<SecretString>,
}
```

`skip_serializing` costs nothing here: a secret has no default worth printing, and
`#[config(secret)]` renders `<redacted>` in place of one anyway. The key keeps its row, its
spellings and its `secret` flag; only the observed value is left out, which is where it belongs.

When the type is not yours to annotate, `Schema::with_defaults_from_value` takes an already-built
`figment::value::Value` and asks for no `Serialize` bound on the root at all.

**3. Generate**, from the crate that owns the root type:

```bash
cargo run --example config-schema -- --format markdown        > docs/config.md
cargo run --example config-schema -- --format markdown-loader >> docs/config.md
cargo run --example config-schema -- --format json            > docs/config.json
cargo run --example config-schema -- --format toml            > config.example.toml
cargo run --example config-schema -- --format json-schema     > config.schema.json
```

Three markdown renderings, because a page wants different combinations of two tables: `markdown` is
both, `markdown-loader` is the handful of variables that *select* the layers — `<PREFIX>CONFIG`,
`<PREFIX>SECRETS_DIR`, anything reserved — and `markdown-keys` is the configuration keys alone.
`--only` slices the two that carry keys and is refused for `markdown-loader`, which has none.

In a workspace, `-p` picks the member: `cargo run -p myservice --example config-schema`. One
generator per *dialect*: two binaries reading one prefix are one document, joined with
`Schema::merge`, while two binaries with two prefixes are two schemas and merging them is refused.

**4. Fail the build when the checked-in copy goes stale:**

```yaml
- run: cargo run --example config-schema -- --format markdown > docs/config.md
- name: Configuration reference is current
  run: git diff --exit-code -- docs/config.md
```

That is the whole point of generating it: the table cannot drift from the code, because a pull
request that changes a key without regenerating fails.

## Keeping it out of the production build

`serde_json` is linked and the derive costs compile time, which for a service that ships in a
container may be worth avoiding. Put the whole thing behind a feature of your own — but gate three
things, not one: the derive, the `#[config(...)]` helper attributes (or the build fails with
`cannot find attribute config` the moment the derive is not applied), and `derive(Serialize)`,
which the loader never needs and `with_defaults_from` does:

```toml
[features]
config-schema = ["terrace-config/schema-cli"]

[dependencies]
terrace-config = { git = "…", tag = "…" }   # no `schema` here

[[example]]
name = "config-schema"
required-features = ["config-schema"]
```

```rust
#[derive(Deserialize, Default)]
#[cfg_attr(
    feature = "config-schema",
    derive(serde::Serialize, terrace_config::schema::Describe)
)]
struct Github {
    /// Bearer token lifting the GitHub API rate limit.
    #[cfg_attr(feature = "config-schema", config(secret))]
    #[serde(skip_serializing)]
    token: Option<SecretString>,
}
```

Both derives go in one `cfg_attr`, because both exist for the same job: `Describe` reports the
keys and `Serialize` is what `with_defaults_from` reads the `Default` column out of. A config
struct that is only ever deserialised has no other reason to carry `Serialize`, so leaving it
ungated links serde's serialiser into the production build for nothing.

`#[serde(skip_serializing)]` stays ungated: it is inert without a `Serialize` impl, and a
`cfg_attr` around it would gate the one attribute that has to agree with the field's type either
way.

Then `cargo run --features config-schema --example config-schema`. The `cfg_attr` on every
`#[config(...)]` is the price; a struct with no `#[config(...)]` attributes needs only the two
derives gated.

## What the columns mean

The `Default` column carries the observed value with its `#[config(note = "…")]` prose in
parentheses — `` `0` (permanent) `` — because the two answer different questions: `0` is what an
operator compares against what they set, and "permanent" is why they would leave it alone. The
JSON keeps them as separate `default` and `note` fields, and `Column::DefaultValue` plus
`Column::Note` splits them into two Markdown columns.

A default that is a secret renders `<redacted>` regardless of what the value is, and a *required*
key reports no default at all — whatever `Default` put in the field is an artefact of building
the value, and printing it would tell an operator they can leave the key out.
