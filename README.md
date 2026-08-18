<!--
Generated from .github/templates/README.md.hbs — edit that file, not this one. CI renders it on
every pull request and commits the result back to the branch; a push to main whose README.md
does not match its template fails the `readme` check in .github/workflows/docs.yml.

Variables come from .github/scripts/readme-variables.sh, which reads Cargo.toml:

    version  the [package] version, e.g. 0.1.0
    tag      the same with a leading v, e.g. v0.1.0
    msrv     the rust-version, e.g. 1.94

That is what keeps the install snippet and the MSRV badge correct across a release: the release
pull request is the commit that changes those numbers, so it arrives with the rendered README
already updated.
-->
# terrace-config

[![CI](https://github.com/TimSchoenle/terrace-config/actions/workflows/ci.yml/badge.svg)](https://github.com/TimSchoenle/terrace-config/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/tag/TimSchoenle/terrace-config?label=version&sort=semver&color=blue)](https://github.com/TimSchoenle/terrace-config/tags)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](Cargo.toml)
[![Licence](https://img.shields.io/badge/licence-MIT-blue)](LICENSE)

Layered [figment](https://docs.rs/figment) configuration for services that read their secrets
from files — Kubernetes `Secret` volumes, Docker `_FILE` indirection — plus a supervisor that
rebuilds the service when those files change.

```toml
[dependencies]
terrace-config = { git = "https://github.com/TimSchoenle/terrace-config", tag = "v0.4.0" }
```

Pin by tag, not branch. `Cargo.lock` records the resolved revision either way, but a branch
dependency lets `cargo update` move silently across arbitrary commits, whereas a tag makes every
bump a deliberate manifest edit that shows up in review.

## Quick start

Every environment name derives from a single prefix.

```rust
use serde::Deserialize;
use terrace_config::Terrace;

#[derive(Deserialize)]
struct Config {
    database: Database,
}

#[derive(Deserialize)]
struct Database {
    url: String,
    #[serde(default = "default_pool")]
    max_connections: u32,
}

fn default_pool() -> u32 { 16 }

let config: Config = Terrace::new("MYAPP_").load()?;
```

`Terrace::new("MYAPP_")` reads `MYAPP_CONFIG`, `MYAPP_SECRETS_DIR`, `MYAPP_*` and
`MYAPP_<KEY>_FILE`.

## The layers

Lowest precedence first:

| # | Layer | Source | Example |
|---|-------|--------|---------|
| 1 | Struct defaults | your `serde` attributes | `#[serde(default = "…")]` |
| 2 | TOML | `$MYAPP_CONFIG`, defaulting to `./config.toml` | `[database]`<br>`url = "postgres://localhost/app"` |
| 3 | Environment | `MYAPP_`-prefixed, `__`-nested | `MYAPP_DATABASE__URL=…` |
| 4 | Secrets directory | every key-named file in `$MYAPP_SECRETS_DIR` | `/run/secrets/database__url` |
| 5 | File indirection | `MYAPP_<KEY>_FILE=/path` | `MYAPP_DATABASE__URL_FILE=/run/secrets/db` |

All five spell the same field the same way: `__` separates nesting levels and case is folded, so
`database.url` is `MYAPP_DATABASE__URL` as a variable and `database__url` as a file name.

If `$MYAPP_CONFIG` names a **directory**, every `*.toml` directly inside it is merged in sorted
order — so a mounted `ConfigMap` containing `10-base.toml` and `20-overrides.toml` merges the way
an operator reading the mount would predict. A missing config file is not an error; running with
no file at all is the normal development case.

## Examples

### Reading a Kubernetes `Secret` volume

Given a `Secret` mounted at `/run/secrets`:

```
/run/secrets/
├── ..2026_08_02_10_00_00/
│   └── database__url          # postgres://real/app
├── ..data -> ..2026_08_02_10_00_00
└── database__url -> ..data/database__url
```

Set `MYAPP_SECRETS_DIR=/run/secrets` and `config.database.url` is filled from the symlink. The
provider follows the `..data` indirection and skips the dot-prefixed entries, so the layer works
against a real projected volume rather than only against a directory of plain files.

### Reserving keys the program reads itself

A key your program reads straight from the environment cannot be supplied by a file — the layers
do not exist yet at that point. Declaring it makes the attempt an error instead of a silent
no-op:

```rust
let config: Config = Terrace::new("MYAPP_")
    .reserve("MYAPP_PROFILE")
    .load()?;
```

`MYAPP_CONFIG` and `MYAPP_SECRETS_DIR` are reserved automatically, since both are read to decide
what the layers are.

### Renaming the environment variables

Every derived name is overridable:

```rust
let layers = Terrace::new("MYAPP_")
    .config_var("MYAPP_CONFIG_PATH")     // instead of MYAPP_CONFIG
    .secrets_dir_var("CREDENTIALS_DIR")  // instead of MYAPP_SECRETS_DIR
    .default_config_path("/etc/myapp/config.toml")
    .file_suffix("_PATH")                // instead of _FILE
    .nesting_separator("_");             // instead of __
```

### Choosing what happens when two sources define one key

By default a key supplied by more than one of the last three layers fails the load:

```rust
use terrace_config::{ShadowPolicy, Terrace};

let layers = Terrace::new("MYAPP_").shadow_policy(ShadowPolicy::LastWins);
```

`ShadowPolicy::Reject` (the default) refuses to boot. `ShadowPolicy::LastWins` resolves by
precedence — environment, then secrets directory, then `_FILE` — which is what
`figment_file_provider_adapter` does, and is there so the crate is adoptable mid-migration.

The reason `Reject` is the default: a stale `MYAPP_DATABASE__PASSWORD` shadowing a mounted secret
that has since been rotated leaves the service working on the old credential, and the discrepancy
surfaces during an incident rather than during a deploy.

### Dropping to the raw figment

```rust
let figment = Terrace::new("MYAPP_").figment()?;
let profile: String = figment.extract_inner("profile")?;
```

`figment` is re-exported, so you can name its types without adding it to your own manifest.

## Reloading

A `Secret` or `ConfigMap` mounted as a volume is updated in place by the kubelet: a new
timestamped directory is written and `..data` is renamed over the old one. That is the only way a
long-lived process learns a credential was rotated, since environment variables are fixed for the
life of a process.

`reload::run` takes the closure that builds your whole runtime and re-runs it whenever the
watched directories change and then go quiet:

```rust,ignore
use std::sync::Arc;
use terrace_config::Terrace;
use tokio_util::sync::CancellationToken;

fn layers() -> Terrace {
    Terrace::new("MYAPP_")
}

#[tokio::main]
async fn main() -> Result<(), ServiceError> {
    let boot = layers().load_watched::<Config>()?;
    let shutdown = CancellationToken::new();

    terrace_config::reload::run(
        (boot.value, boot.sources),
        &shutdown,
        // Called once per debounced change.
        || {
            layers()
                .load_watched::<Config>()
                .map(|loaded| (loaded.value, loaded.sources))
                .map_err(ServiceError::from)
        },
        // Called once per generation, with a token cancelled when this one must stop.
        |config: Arc<Config>, token: CancellationToken| serve(config, token),
    )
    .await
}
```

Your error type needs `From<reload::WatchError>` and `Display`; nothing else:

```rust,ignore
#[derive(Debug, thiserror::Error)]
enum ServiceError {
    #[error("{0}")]
    Watch(#[from] terrace_config::reload::WatchError),
    #[error("configuration: {0}")]
    Config(String),
}
```

Behaviour worth knowing:

- **`build` must return once it has stopped.** The replacement is not built until the old future
  completes, so the previous listener has released its address before the new one binds it.
- **Everything `build` constructs is rebuilt** — pool, state, router, listener, background tasks.
  Process-global installations made before `run` (a `tracing` subscriber, a metrics recorder) are
  not, and changing the configuration that drives those still needs a restart.
- **A failed or no-op reload changes nothing.** If the new configuration cannot be loaded, or
  resolves to the same values already running, the running service is left exactly as it is and
  the reason is logged.
- **Changes are debounced** for 500 ms by default, since one logical volume update fires several
  filesystem events. Use `reload::run_with` and `reload::Debounce` to choose another window.

Change detection compares the merged figment value, not your config struct — a struct holding a
`secrecy::SecretString` cannot implement `PartialEq` at all.

## Testing your configuration

Every service that loads configuration this way ends up writing the same fixture: a temporary
directory, a secrets file in it, an environment variable pointing at the directory, and a way to
put all of it back afterwards. The `testing` feature is that fixture, written once.

```toml
[dev-dependencies]
terrace-config = { git = "…", tag = "v0.4.0", features = ["testing"] }
```

```rust,ignore
use terrace_config::{Terrace, testing::Harness};

fn harness() -> Harness {
    Harness::over(Terrace::new("MYAPP_").reserve("MYAPP_PROFILE"))
}

#[test]
fn the_mounted_secret_outranks_the_config_map() {
    harness().run(|jail| {
        jail.config("[database]\nurl = \"postgres://placeholder/app\"\n")?;
        jail.secret("database__url", "postgres://real/app\n")?;

        let config: Config = jail.load()?;
        assert_eq!(config.database.url.expose_secret(), "postgres://real/app");
        Ok(())
    });
}
```

Inside `run`: the temporary directory is the working directory and is deleted when the test
returns, the environment starts empty and is restored afterwards, and jails are serialised
process-wide because the environment is a global.

The closure returns **this crate's own `Error`**. That is the point of the type rather than a
detail. The obvious way to write this fixture is around `figment::Jail`, whose closure returns a
`figment::Error` — a type large enough that clippy's `result_large_err` fires on every test file
that uses it, and which has no `From<std::io::Error>`, so arranging a symlink means converting an
error by hand. Here `?` works on the arrangement and on the load being tested alike.

### What it arranges

Each method sets *both* halves of a layer — the file, and the variable that makes the loader read
it:

| Method | The layer it arranges |
|--------|-----------------------|
| `jail.config(toml)` | TOML at `$MYAPP_CONFIG`, as a single file |
| `jail.fragment(name, toml)` | TOML at `$MYAPP_CONFIG`, as a directory of fragments merged in name order |
| `jail.env_key("auth.jwt_secret", v)` | `MYAPP_AUTH__JWT_SECRET` |
| `jail.secret("auth__jwt_secret", v)` | a key-named file in `$MYAPP_SECRETS_DIR` |
| `jail.indirection("auth.jwt_secret", v)` | `MYAPP_AUTH__JWT_SECRET_FILE`, pointing at a file it writes |

Every name is derived from the loader the harness was built over, never restated. A test that
spells `MYAPP_AUTH__JWT_SECRET_FILE` out by hand keeps passing after `Terrace::file_suffix`
renames the mechanism — while testing a variable the loader no longer reads.

Below that sit `jail.env`, `jail.write`, `jail.create_dir` and `jail.path` for anything the named
methods do not cover, and `jail.terrace()` for a loader with one knob changed:

```rust,ignore
let config: Config = jail.terrace().shadow_policy(ShadowPolicy::LastWins).load()?;
```

### Mounted volumes

A `Volume` builds the shapes Kubernetes actually produces, which is what the three layouts are
for:

```rust,ignore
jail.secrets_volume()
    .file("auth__jwt_secret", "from-the-volume")
    .stray_dir("nested")
    .projected()          // `..data` and a generation directory beside the real keys
    .create()?;
```

- `.plain()` — ordinary files in an ordinary directory. The default.
- `.projected()` — the *names* a projected volume has, with the keys as regular files. Portable,
  and what pins the skipping rules.
- `.symlinked()` — the mount as the kubelet writes it: the keys are symlinks to `..data/<key>`,
  and `..data` is a symlink to the generation directory. Unix only, so a test using it carries
  `#[cfg(unix)]`.

The last two are not stylistic variants. `.projected()` stayed green while every service in a
cluster booted on compiled defaults, because `DirEntry::metadata()` does not follow symlinks and
reported every real key as "not a file". Only `.symlinked()` reproduces that.

`jail.config_volume()` is the same builder wired to `$MYAPP_CONFIG`, and `jail.volume(dir)` is
one wired to nothing — a decoy, for asserting that a *renamed* variable is the only one being
read.

### Reloading

With `reload` on as well, `Rebuilds` records what the supervisor handed the build closure:

```rust,ignore
harness().run(|jail| {
    jail.secret("database__url", "postgres://one/app")?;
    let boot = jail.load_watched::<Config>()?;
    let loader = jail.terrace();
    let files = jail.sandbox();          // a handle a spawned task can hold
    let rebuilds: Rebuilds = Rebuilds::new();

    jail.block_on(async {
        let shutdown = CancellationToken::new();
        let driver = rebuilds.clone();
        let stop = shutdown.clone();

        tokio::spawn(async move {
            driver.wait_for(1).await;
            files.write("secrets/database__url", "postgres://two/app").expect("rotate");
            driver.wait_for(2).await;
            stop.cancel();
        });

        terrace_config::reload::run(
            (boot.value, boot.sources),
            &shutdown,
            || loader.load_watched().map(|l| (l.value, l.sources)).map_err(ServiceError::from),
            rebuilds.serving(|c: &Config| c.database.url.expose_secret().to_owned()),
        )
        .await
        .expect("the supervisor returns when shutdown is cancelled");
    });

    assert_eq!(rebuilds.seen(), ["postgres://one/app", "postgres://two/app"]);
    Ok(())
});
```

`rebuilds.stays_at(2, window)` is the other assertion, and the one most supervisor tests are
really making: a reload that fails to load, or that resolves to the values already running, must
leave the running service alone. `ServiceError` is the error type `reload::run` asks a service
for, and `rebuilds.serving(…)` is a build closure that records and then serves until it is
cancelled — returning early instead looks right and ends the supervisor, so the test would pass
its first assertion and never see a reload.

## Feature flags

| Feature | Contents | Dependencies |
|---------|----------|--------------|
| `loader` (default) | `Terrace`, the three providers, `Loaded`/`Sources` | `figment` |
| `reload` | `reload::run`, the rebuild supervisor | `tokio`, `notify`, `tracing` |
| `schema` | `schema::Schema`, the configuration dump | `serde_json`, `syn` |
| `testing` | `testing::Harness`, the sandbox above | `figment/test` |
| `full` | the three runtime sets | all of it |

`loader` and `reload` are independent. `reload` does not depend on `loader`: it works for anyone with a
`Fn(Arc<C>, CancellationToken) -> Future` and a way to detect change, and requiring figment would
narrow that for no benefit. Symmetrically, a service that only reads a config file at boot has no
reason to link tokio and notify:

```toml
terrace-config = { git = "…", tag = "v0.4.0", default-features = false, features = ["loader"] }
```

The single line joining those two is `impl reload::Source for Sources`, compiled only when both
features are on. `schema` is the exception to the independence: it is an add-on to `loader`,
because every spelling it reports is derived from the loader's dialect, and so is `testing`.

`testing` is deliberately outside `full`: it belongs in `[dev-dependencies]`, and a service
asking for everything this crate does at runtime should not link a test harness.

## Generating the configuration reference

The loader never learns the shape of a config — it hands the merged figment to `serde` and takes
back a `T`. The `schema` feature inverts that, so the reference table every service needs is
generated from the type instead of maintained beside it.

```toml
terrace-config = { git = "…", tag = "v0.4.0", features = ["schema"] }
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

### Two outputs

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

### A whole crate, a whole workspace, or one subsystem

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

### Wiring it into your own crate

Nothing here reads the environment, so a documentation job produces the same answer on a runner
where none of the variables it describes are set. `examples/config-schema.rs` in this repository
is the whole pattern; what follows is that pattern as it looks in *your* project.

**1. Take the feature.** The derive is used on your config structs, so it is a normal dependency,
not a dev-dependency:

```toml
[dependencies]
terrace-config = { git = "…", tag = "v0.4.0", features = ["schema"] }

# The generator, so `cargo clippy --all-targets` keeps it compiling.
[[example]]
name = "config-schema"
```

**2. Add the generator**, `examples/config-schema.rs`, next to the root config type:

```rust
use std::process::ExitCode;
use myservice::Config;              // the root; nested types need only `Describe`
use terrace_config::Terrace;

fn main() -> ExitCode {
    let markdown = std::env::args().any(|a| a == "--markdown");
    let schema = Terrace::new("MYSERVICE_")
        .reserve("MYSERVICE_PROFILE")
        .schema::<Config>()
        .with_defaults_from(&Config::default())
        .expect("the default config serialises");

    println!("{}", if markdown { schema.to_markdown() } else { schema.to_json().unwrap() });
    ExitCode::SUCCESS
}
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
cargo run --example config-schema -- --markdown > docs/config.md
cargo run --example config-schema              > docs/config.json
```

In a workspace, `-p` picks the member: `cargo run -p myservice --example config-schema`. One
generator per *dialect*: two binaries reading one prefix are one document, joined with
`Schema::merge`, while two binaries with two prefixes are two schemas and merging them is refused.

**4. Fail the build when the checked-in copy goes stale:**

```yaml
- run: cargo run --example config-schema -- --markdown > docs/config.md
- name: Configuration reference is current
  run: git diff --exit-code -- docs/config.md
```

That is the whole point of generating it: the table cannot drift from the code, because a pull
request that changes a key without regenerating fails.

### Keeping it out of the production build

`serde_json` is linked and the derive costs compile time, which for a service that ships in a
container may be worth avoiding. Put the whole thing behind a feature of your own — but gate three
things, not one: the derive, the `#[config(...)]` helper attributes (or the build fails with
`cannot find attribute config` the moment the derive is not applied), and `derive(Serialize)`,
which the loader never needs and `with_defaults_from` does:

```toml
[features]
config-schema = ["terrace-config/schema"]

[dependencies]
terrace-config = { git = "…", tag = "v0.4.0" }   # no `schema` here

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

### What the columns mean

The `Default` column carries the observed value with its `#[config(note = "…")]` prose in
parentheses — `` `0` (permanent) `` — because the two answer different questions: `0` is what an
operator compares against what they set, and "permanent" is why they would leave it alone. The
JSON keeps them as separate `default` and `note` fields, and `Column::DefaultValue` plus
`Column::Note` splits them into two Markdown columns.

A default that is a secret renders `<redacted>` regardless of what the value is, and a *required*
key reports no default at all — whatever `Default` put in the field is an artefact of building
the value, and printing it would tell an operator they can leave the key out.

## Secrets and `Debug`

Two public types hold secret material: `provider::FileValue` and `Sources`, whose fingerprint is
every configuration value merged together. Neither derives `Debug` — both print `<redacted>` in
place of the value, so logging one is never a way to leak a credential. No error in this crate
prints a value either.

## Compared with

| Crate | What it gives you | What it lacks |
|-------|-------------------|---------------|
| [`figment`](https://crates.io/crates/figment) | the layering engine this builds on | no secrets-directory provider, no reload |
| [`figment_file_provider_adapter`](https://github.com/nitnelave/figment_file_provider_adapter) | the `_FILE` suffix half | no secrets directory; resolves a doubly-supplied key by precedence; last released October 2023 |
| [`confique`](https://github.com/LukasKalbertodt/confique) | a well-maintained figment alternative | no reload |
| [`settings_loader`](https://github.com/dmrolfs/settings-loader-rs) | almost this precedence order, and `.with_secrets(path)` | hot reload is a listed future enhancement, not a feature |
| [`hot_reload`](https://github.com/junkurihara/rust-hot-reloader) | the closest single match | requires `V: Eq + PartialEq`, which a config holding a `SecretString` cannot satisfy; its file reloader watches files non-recursively, which misses a volume remount |

## Contributing

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org): the type
decides both the changelog section and the version bump release-please proposes. `feat` and a
breaking change move the minor while the crate is pre-1.0; `fix` moves the patch.

`README.md` is generated. Edit `.github/templates/README.md.hbs` instead — CI renders it on every
pull request and commits the result back to the branch, and a push to `main` whose `README.md`
does not match its template fails.

The gates a pull request has to pass are in [`.github/workflows/ci.yml`](.github/workflows/ci.yml);
all of them run locally:

```bash
cargo fmt --all --check
cargo clippy --all-features --all-targets
cargo test --all-features
cargo deny check
cd fuzz && cargo test          # replays every committed seed and corpus entry
```

## Licence

MIT. See [LICENSE](LICENSE).
