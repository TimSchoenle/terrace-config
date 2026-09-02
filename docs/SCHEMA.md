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
| `#[config(element)]` | Report the shape of one element of a container-typed key |
| `#[config(element_values)]` | Report the values one element of a container-typed key accepts |
| `#[config(note = "…")]` | Annotate the observed default with prose |
| `#[config(skip)]` | Omit the key without affecting deserialisation |

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

- **It is opt-in, and a container that does not take it up is unchanged.** A field whose element is
  deliberately a leaf — a map keyed by operator-chosen names, whose values are not a documented
  shape — keeps publishing exactly the bytes it published before.
- **The element type has to be spelled out.** A derive has only tokens, so
  `type Routes = Vec<RouteConfig>` is a bare identifier here and is rejected with an error saying
  so rather than guessed at. Spell the container out, or implement `Describe` by hand and call
  `Sink::repeated`.
- **`element_values` reports what the derive reports, which is serde's default wire form.** A type
  whose `Deserialize` is something else — `#[serde(try_from = "String")]` over a case-insensitive
  `FromStr` is the one that turns up — accepts spellings that are not in the variant list, and a
  schema listing only the variants would reject a file the loader takes. Leave such an element
  undescribed; that is the one thing this crate will not publish.

The environment layer is untouched by any of this. A container is still supplied as one TOML
literal, so `text_form` stays `structured` and `text_constraint` stays the bracket pattern — the
element lives in document space only.

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
