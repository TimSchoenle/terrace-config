//! Layered [figment](https://docs.rs/figment) configuration that survives mounted-secret
//! rotation — Kubernetes `Secret` volumes, `_FILE` indirection, and a supervisor that rebuilds
//! your service when they change.
//!
//! # The two feature sets
//!
//! | Feature | What it is | What it costs |
//! |---|---|---|
//! | `loader` (default) | `Terrace`, the three providers, `Loaded`/`Sources` | `figment` |
//! | `reload` | `reload::run`, a supervisor that rebuilds a runtime on change | `tokio`, `notify`, `tracing` |
//! | `full` | both | both |
//!
//! They are independent on purpose. `reload` does not depend on `loader`: it is useful to
//! anyone with a `Fn(Arc<C>, CancellationToken) -> Future` and a way to detect change, and
//! requiring figment would shrink its audience for no benefit. Symmetrically, a service that
//! only reads a config file at boot has no reason to link tokio and notify. The single line
//! joining them is `impl Source for Sources`, which exists only when both are on.
//!
//! This module's documentation is itself feature-gated, so the sections below describe only
//! what you actually compiled.

// The rest of the crate documentation links to items that exist only under one feature or the
// other, and `broken_intra_doc_links` is denied. Written as gated `doc` attributes rather than
// `//!` comments so that `cargo doc --no-default-features --features reload` is not a wall of
// unresolved links.
#![cfg_attr(
    feature = "loader",
    doc = r#"
# The layers

Lowest precedence first, for `Terrace::new("MYAPP_")`:

1. Your struct's `serde` defaults.
2. TOML at `$MYAPP_CONFIG` — a file, or every `*.toml` in it when it names a directory.
3. `MYAPP_`-prefixed, `__`-nested environment variables.
4. Every key-named file in `$MYAPP_SECRETS_DIR`.
5. `MYAPP_<KEY>_FILE=/path` indirection.

Each name is derived from the one prefix, and each is overridable: see [`Terrace::config_var`],
[`Terrace::secrets_dir_var`], [`Terrace::file_suffix`] and [`Terrace::nesting_separator`].

```no_run
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

// `MYAPP_PROFILE` is read straight from the environment elsewhere in the program, so a file
// naming it is refused rather than silently ignored.
let config: Config = Terrace::new("MYAPP_").reserve("MYAPP_PROFILE").load()?;
# Ok::<(), terrace_config::Error>(())
```

# What makes it different

Three properties, each of which exists because it failed in production first:

- **A secrets *directory* provider that survives a projected Kubernetes volume.** `..data`
  symlink traversal, dot-prefix skipping, and `fs::metadata` rather than
  `DirEntry::metadata()` — which despite its name does not follow symlinks, and so reports
  every real key as "not a file" and yields a silently empty layer. See
  [`provider::SecretsDir::read`].
- **Shadow-key *rejection* rather than precedence.** A stale environment variable shadowing a
  rotated mounted secret keeps the service running on the old credential, and the discrepancy
  surfaces during an incident rather than during a deploy. See [`ShadowPolicy`].
- **Value-fingerprint no-op detection over a struct containing non-`PartialEq` secret types.**
  [`Sources::differs_from`] compares the merged figment value, not the typed config, so a
  `..data` swap that moved no key does not rebuild anything.

# Secrets and `Debug`

Two public types hold secret material: [`provider::FileValue`] and [`Sources`], whose
fingerprint is every configuration value merged together. Neither derives [`Debug`] — both
print `<redacted>` in place of the value, so that logging one is never a way to leak a
credential. No error in this crate prints a value either.
"#
)]
#![cfg_attr(
    feature = "reload",
    doc = r"
# Reloading

[`reload::run`] takes the closure that builds your whole runtime and re-runs it whenever the
watched directories change and then go quiet. A reload that cannot be loaded, or that resolves
to the values already running, leaves the running service exactly as it is. See the
[`reload`] module for the failure posture in full.
"
)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "loader")]
mod dialect;
#[cfg(feature = "loader")]
mod error;
#[cfg(feature = "loader")]
mod layers;
#[cfg(feature = "loader")]
mod loaded;
#[cfg(feature = "loader")]
pub mod provider;
#[cfg(feature = "loader")]
mod terrace;

#[cfg(feature = "reload")]
pub mod reload;

#[cfg(feature = "loader")]
pub use dialect::Dialect;
#[cfg(feature = "loader")]
pub use error::Error;
#[cfg(feature = "loader")]
pub use layers::ShadowPolicy;
#[cfg(feature = "loader")]
pub use loaded::{Loaded, Sources};
#[cfg(feature = "loader")]
pub use terrace::Terrace;

// Re-exported so a consumer can name the type in a signature without adding figment to their
// own manifest — `Terrace::figment` returns one.
#[cfg(feature = "loader")]
pub use figment;
