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
terrace-config = { git = "https://github.com/TimSchoenle/terrace-config", tag = "v0.2.0" }
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

## Feature flags

| Feature | Contents | Dependencies |
|---------|----------|--------------|
| `loader` (default) | `Terrace`, the three providers, `Loaded`/`Sources` | `figment` |
| `reload` | `reload::run`, the rebuild supervisor | `tokio`, `notify`, `tracing` |
| `full` | both | both |

The two are independent. `reload` does not depend on `loader`: it works for anyone with a
`Fn(Arc<C>, CancellationToken) -> Future` and a way to detect change, and requiring figment would
narrow that for no benefit. Symmetrically, a service that only reads a config file at boot has no
reason to link tokio and notify:

```toml
terrace-config = { git = "…", tag = "v0.2.0", default-features = false, features = ["loader"] }
```

The single line joining the halves is `impl reload::Source for Sources`, compiled only when both
features are on.

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
