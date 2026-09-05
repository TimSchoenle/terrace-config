<!--
Generated from .github/templates/README.md.hbs — edit that file, not this one.

CI renders it on every pull request and commits the result back to the branch. A push to `main`
whose README.md does not match its template fails the `readme` job in
.github/workflows/docs.yml, which is a required check.

The payload is collected by TimSchoenle/actions/actions/common/readme-variables, which reads
Cargo.toml and walks docs/, merged over the output of one command:

    bash .github/scripts/readme-variables.sh

Every number this page quotes about itself — the tag its install snippets pin, the MSRV its badge
shows, the licence its last section names — comes from that payload, so the release pull request
is the commit that corrects them.

Nothing in this comment may contain a mustache that is not a real reference.
-->

# terrace-config

Layered figment configuration that survives mounted-secret rotation.

[![Release](https://img.shields.io/github/v/release/TimSchoenle/terrace-config?sort=semver)](https://github.com/TimSchoenle/terrace-config/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/TimSchoenle/terrace-config/ci.yml?branch=main&label=ci)](https://github.com/TimSchoenle/terrace-config/actions/workflows/ci.yml)
[![Licence](https://img.shields.io/github/license/TimSchoenle/terrace-config)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](Cargo.toml)

## What this is

Five layers resolve one configuration: your struct's `serde` defaults, a TOML file or directory,
prefixed environment variables, a directory of key-named files, and `_FILE` indirection. Every
name in all five is derived from one prefix.

The last two exist because a Kubernetes `Secret` arrives as a directory of files rather than as
environment variables. The kubelet replaces that directory in place when the secret is rotated,
and environment variables are fixed for the life of a process, so the `reload` feature rebuilds
whatever the configuration built when those files change.

The crate is a git dependency rather than a crates.io release. `publish = false` in the manifest
stops an accidental publish while leaving the name reserved, so a consumer pins a tag.

## Quick start

```toml
[dependencies]
terrace-config = { git = "https://github.com/TimSchoenle/terrace-config", tag = "v0.10.1" }
```

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

## Table of contents

- [Features](#features)
- [Installation](#installation)
- [Usage](#usage)
- [Configuration](#configuration)
- [Compatibility](#compatibility)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [Security](#security)
- [Licence](#licence)

## Features

- **A secrets directory that survives a projected volume.** The provider follows the `..data`
  symlink, skips the dot-prefixed entries, and calls `fs::metadata` rather than
  `DirEntry::metadata()`, which does not follow symlinks and so reports every real key as "not a
  file".
- A key supplied by two of the last three layers fails the load. `ShadowPolicy::LastWins` resolves
  by precedence instead, for a codebase migrating onto this one.
- Change detection compares the merged figment value rather than the typed config, so a `..data`
  swap that moved no key rebuilds nothing, and a config holding a `secrecy::SecretString` needs no
  `PartialEq`.
- `Terrace::explain` reports which layer supplied each key and what that key shadowed, while
  holding no configuration value itself.
- The `Describe` derive turns the config types into a reference table, a `config.example.toml`, a
  JSON Schema, and a contract document an image carries so a Helm chart can be checked against it.
- `testing::Harness` writes both halves of every layer into a temporary directory and puts the
  environment back afterwards.
- Five fuzz oracles cover the loader, each panicking when the loader breaks a rule. CI replays
  every committed seed and corpus entry on a plain `cargo test`, then runs a libFuzzer campaign.

## Installation

```toml
[dependencies]
terrace-config = { git = "https://github.com/TimSchoenle/terrace-config", tag = "v0.10.1" }
```

Pin by tag, not branch. `Cargo.lock` records the resolved revision either way, but a branch
dependency lets `cargo update` move silently across arbitrary commits, whereas a tag makes every
bump a deliberate manifest edit that shows up in review.

`loader` is the only feature on by default. Take the rest by name:

```toml
terrace-config = { git = "…", tag = "v0.10.1", features = ["reload", "explain"] }
```

A service that reads a config file at boot and never reloads turns the default off, and links
neither tokio nor notify:

```toml
terrace-config = { git = "…", tag = "v0.10.1", default-features = false, features = ["loader"] }
```

## Usage

### The layers

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

Layer 3 carries only the variables that are *values*. `$MYAPP_CONFIG`, `$MYAPP_SECRETS_DIR`,
anything [reserved](#reserving-keys-the-program-reads-itself) and every `MYAPP_<KEY>_FILE` are this
loader's own mechanism rather than configuration, and none of them is offered to your type — so a
root deriving `#[serde(deny_unknown_fields)]` loads rather than failing on a field it never
declared.

If `$MYAPP_CONFIG` names a **directory**, every `*.toml` directly inside it is merged in sorted
order, so a mounted `ConfigMap` containing `10-base.toml` and `20-overrides.toml` merges the way
an operator reading the mount would predict. A missing config file is not an error; running with
no file at all is the normal development case.

### Reading a Kubernetes `Secret` volume

Given a `Secret` mounted at `/run/secrets`:

```text
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

A key your program reads straight from the environment cannot be supplied by a file, because the
layers do not exist yet at that point. Declaring it makes the attempt an error instead of a silent
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
precedence, which is what `figment_file_provider_adapter` does, and is there so the crate is
adoptable mid-migration. That precedence is environment, then secrets directory, then `_FILE`.

The reason `Reject` is the default: a stale `MYAPP_DATABASE__PASSWORD` shadowing a mounted secret
that has since been rotated leaves the service working on the old credential, and the discrepancy
surfaces during an incident rather than during a deploy.

### Dropping to the raw figment

```rust
let figment = Terrace::new("MYAPP_").figment()?;
let profile: String = figment.extract_inner("profile")?;
```

`figment` is re-exported, so you can name its types without adding it to your own manifest.

## Configuration

Everything past the loader sits behind a feature, because the parts differ in dependency weight
and in audience.

| Feature | Contents | Cost |
|---------|----------|------|
| `loader` (default) | `Terrace`, the three providers, `Loaded`/`Sources` | `figment` |
| `reload` | `reload::run`, the rebuild supervisor | `tokio`, `notify`, `tracing` |
| `schema` | `schema::Schema` and the `Describe` derive | `serde_json`, the macro crate |
| `schema-cli` | `schema::cli`, the `--format` generator program | nothing `schema` did not already pull |
| `explain` | `Terrace::explain`, which layer supplied each key | nothing |
| `testing` | `testing::Harness`, the jail | `figment/test` |
| `full` | every runtime set above | all of it |

`loader` and `reload` are independent, and `reload` must not be made to depend on `loader`. It is
useful to anyone with a `Fn(Arc<C>, CancellationToken) -> Future` and a way to detect change, and
requiring figment would narrow that audience for no benefit. The single line joining the two is
`impl reload::Source for Sources`, compiled only when both features are on.

`schema`, `schema-cli`, `explain` and `testing` are add-ons to `loader`, because every spelling
each of them reports is derived from the loader's dialect. `explain` is the one feature that costs
no dependency at all; it is a feature anyway, because what it carries is a second walk of every
layer and the strings that render it, dead weight in an image that will never print a report.

`testing` is deliberately outside `full`. It belongs in `[dev-dependencies]`, and a service asking
for everything this crate does at runtime should not link a test harness.

## Compatibility

| | Supported |
| --- | --- |
| Rust | 1.94, checked by the `msrv` job (edition 2024) |
| Platforms | Linux and Windows, both in the test matrix |
| `Volume::symlinked` | Unix only; a test using it carries `#[cfg(unix)]` |

## Documentation

| Document | Purpose |
| --- | --- |
| [Compared with other crates](docs/ALTERNATIVES.md) | The five crates nearest to this one, and what each of them lacks. |
| [Publishing the contract with the image](docs/CONTRACT.md) | A contract is one document, attached to an image digest, saying what configuration the image takes. |
| [Debugging where a value came from](docs/EXPLAIN.md) | The explain feature reports which layer supplied each key, while holding no configuration value. |
| [Reloading](docs/RELOAD.md) | The reload feature rebuilds a running service when the files its configuration came from change. |
| [Generating the configuration reference](docs/SCHEMA.md) | The schema feature derives a reference table, an example file and a JSON Schema from the types. |
| [Testing your configuration](docs/TESTING.md) | The testing feature is a jail that arranges both halves of a layer and restores the environment. |
| [The config contract: image-embedded configuration schemas, validated by the charts](docs/config-contract-plan.md) | A design for shipping each service's configuration surface with its image. |

Outside that table, [fuzz/README.md](fuzz/README.md) covers the oracles, the seed corpus and how
to run a campaign.

## Contributing

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org): the type
decides both the changelog section and the version bump release-please proposes. `feat` and a
breaking change move the minor while the crate is pre-1.0; `fix` moves the patch.

`README.md` is generated. Edit `.github/templates/README.md.hbs` instead. CI renders it on every
pull request and commits the result back to the branch, and a push to `main` whose `README.md`
does not match its template fails.

The gates a pull request has to pass are in [`.github/workflows/ci.yml`](.github/workflows/ci.yml);
all of them run locally:

```bash
cargo fmt --all --check
cargo clippy --all-features --all-targets
cargo test --workspace --all-features
cargo deny check
cd fuzz && cargo test          # replays every committed seed and corpus entry
```

## Security

Two public types hold secret material: `provider::FileValue` and `Sources`, whose fingerprint is
every configuration value merged together. Neither derives `Debug`. Both print `<redacted>` in
place of the value, and no error in this crate prints a value either, so a log line is not a way
to leak a credential. `Terrace::explain` records no value at all, so there is nothing in it to
redact.

[SECURITY.md](SECURITY.md) has the reporting instructions. Do not open a public issue for a
vulnerability.

## Licence

MIT. [LICENSE](LICENSE) has the terms.
