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
//! | `schema` | `schema::Schema`, a machine-readable dump of every key | `serde_json`, `syn` |
//! | `explain` | `Terrace::explain`, which layer supplied each key | nothing |
//! | `testing` | `testing::Harness`, a sandbox for a consumer's own tests | `figment/test` |
//! | `full` | all four | all of it |
//!
//! `full` is the four runtime feature sets. `testing` is not one of them: it belongs in a
//! consumer's `[dev-dependencies]`, and a service asking for everything this crate does at
//! runtime should not link a test harness.
//!
//! `loader` and `reload` are independent on purpose. `reload` does not depend on `loader`: it is useful to
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
    feature = "schema",
    doc = r#"
# Documenting the configuration

The loader never learns the shape of a config: it hands the merged figment to `serde` and takes
back a `T`. [`schema`] inverts that, so the four artefacts every service keeps beside its
configuration — the reference table, the machine-readable contract, the example file, and the
JSON Schema an editor validates against — are generated from the type instead of maintained
beside it.

[`schema::Contract`] is the fifth, and the only one whose reader is not a person or an editor: the
document a container build embeds in its image and attaches to its digest, so that a deployment
pipeline holding nothing but that digest can check the configuration it is about to mount.
Publication is therefore two-sided — [`schema::kube`] names what the *Kubernetes objects* a chart
renders carry, and pairs that with the image's own labels, so the same check a CI job makes against
a rendered file an admission controller can make against a live `ConfigMap`.

```no_run
use serde::{Deserialize, Serialize};
use terrace_config::{Terrace, schema::App, schema::Describe};

#[derive(Deserialize, Serialize, Default, Describe)]
struct Config {
    #[config(nested)]
    csp: Csp,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Csp {
    /// Hash the document's inline scripts instead of allowing `'unsafe-inline'`.
    hash_inline_scripts: bool,
}

let schema = Terrace::new("PORTFOLIO_")
    .schema::<Config>()
    .with_defaults_from(&Config::default())?;

// The contract, for a documentation pipeline to render however it likes:
println!("{}", schema.to_json()?);
// The table itself, ready to paste into a README:
println!("{}", schema.to_markdown());
// The file an operator copies to `config.toml`, commented, with the defaults shown:
println!("{}", schema.to_toml_example());
// A JSON Schema, so an editor completes and validates that file:
println!("{}", schema.to_json_schema()?);
// Both machine-readable halves in one document, for the image to publish:
println!("{}", schema.into_contract(App::new("portfolio")).build()?.to_json()?);
# Ok::<(), terrace_config::Error>(())
```

The `///` comment is the reason this is a derive and not runtime reflection: it is the one column
that exists nowhere but the source.

The two file renderings are the ones worth a `--check`-style diff in CI. A reference table that
has drifted reads wrong; an example file that has drifted is *copied* into a deployment, and a
JSON Schema that has drifted underlines a key that is perfectly valid.
"#
)]
#![cfg_attr(
    feature = "testing",
    doc = r#"
# Testing a service's configuration

Every service that loads configuration this way ends up writing the same fixture: a temporary
directory, a secrets file in it, a variable pointing at the directory, and a way to put all of
it back afterwards. [`testing::Harness`] is that fixture, written once.

```
use terrace_config::{Terrace, testing::Harness};
# #[derive(serde::Deserialize)] struct Config { database: Database }
# #[derive(serde::Deserialize)] struct Database { url: String }

let harness = Harness::over(Terrace::new("MYAPP_").reserve("MYAPP_PROFILE"));

harness.run(|jail| {
    jail.config("[database]\nurl = \"postgres://placeholder/app\"\n")?;
    jail.secret("database__url", "postgres://real/app\n")?;

    let config: Config = jail.load()?;
    assert_eq!(config.database.url, "postgres://real/app");
    Ok(())
});
```

The closure returns this crate's own [`Error`], which is the point: `?` works on the
arrangement and on the load being tested alike, and a test file no longer carries an
`#[expect(clippy::result_large_err, …)]` for a `figment::Error` it never names. See the
[`testing`] module for the whole surface.
"#
)]
#![cfg_attr(
    feature = "explain",
    doc = r#"
# Where did this value come from?

[`Terrace::load`] returns a `T` and throws the provenance away, which is fine right up until a
value arrives from the layer nobody expected. [`Terrace::explain`] keeps it: which layer supplied
each key, which file or variable inside that layer, and which keys more than one layer supplied.

```no_run
use terrace_config::Terrace;

let terrace = Terrace::new("MYAPP_");
// At boot, and again from inside a reload — it re-reads at the moment it is called.
println!("{}", terrace.explain()?);
# Ok::<(), terrace_config::Error>(())
```

An [`Explanation`](explain::Explanation) holds no configuration *value*, only the places values
came from, so printing one into a log that is shipped and retained is safe by construction rather
than safe if reviewed. It is also deliberately harder to break than the load it describes: it
assembles under [`ShadowPolicy::LastWins`] whatever policy is set, so a configuration
[`Terrace::load`] *refuses* can still be explained.
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

#[cfg(feature = "explain")]
pub mod explain;

#[cfg(feature = "schema")]
pub mod schema;

#[cfg(feature = "reload")]
pub mod reload;

// Compiled for this crate's own unit tests as well as for the feature, so that `dialect.rs`
// tests the loader through the same harness a consumer gets rather than through a second,
// private copy of it. `figment::Jail` is available to a `cargo test` build either way: the
// dev-dependency on figment already asks for its `test` feature.
#[cfg(all(feature = "loader", any(feature = "testing", test)))]
pub mod testing;

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
