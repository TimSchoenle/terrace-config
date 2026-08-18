//! A sandbox for testing a service's configuration, and the loader that reads it.
//!
//! Every service that loads configuration this way ends up writing the same test fixture: a
//! temporary directory, a secrets file in it, an environment variable pointing at the directory,
//! and a way to put all of it back afterwards. It is thirty lines that say nothing about the
//! service, it is written once per repository, and each copy drifts.
//!
//! ```
//! use terrace_config::{Terrace, testing::Harness};
//! # #[derive(serde::Deserialize)] struct Config { database: Database }
//! # #[derive(serde::Deserialize)] struct Database { url: String }
//!
//! let harness = Harness::over(Terrace::new("MYAPP_").reserve("MYAPP_PROFILE"));
//!
//! // A mounted secret outranks the `ConfigMap` carrying a placeholder.
//! harness.run(|jail| {
//!     jail.config("[database]\nurl = \"postgres://placeholder/app\"\n")?;
//!     jail.secret("database__url", "postgres://real/app\n")?;
//!
//!     let config: Config = jail.load()?;
//!     assert_eq!(config.database.url, "postgres://real/app");
//!     Ok(())
//! });
//! ```
//!
//! # What the sandbox is
//!
//! [`Harness::run`] gives the closure a [`Jail`]: a temporary directory that is deleted when the
//! test returns, an environment that is empty on the way in and restored on the way out, and the
//! loader the harness was built with. Sandboxes are serialised process-wide, because the
//! environment is a global and a test harness runs tests in parallel by default.
//!
//! The closure returns this crate's [`Error`](crate::Error). That is not a detail: the obvious
//! way to write this fixture is around `figment::Jail`, whose closure returns a `figment::Error`
//! — a type large enough that clippy's `result_large_err` fires on every test file that uses it,
//! and which has no `From<std::io::Error>`, so arranging a symlink means converting an error by
//! hand. Both problems are structural, and both are gone here: `?` works on the arrangement and
//! on the load being tested, in one error type the consuming project already names.
//!
//! # What it arranges
//!
//! Each method sets *both* halves of a layer — the file and the variable that makes the loader
//! read it — and derives every name from the loader under test rather than restating it:
//!
//! | Method | The layer it arranges |
//! |---|---|
//! | [`Jail::config`], [`Jail::fragment`] | TOML at `$<PREFIX>CONFIG`: one file, or a directory of fragments |
//! | [`Jail::env_key`] | a `<PREFIX>`-prefixed, `__`-nested environment variable |
//! | [`Jail::secret`], [`Jail::secret_key`] | a key-named file in `$<PREFIX>SECRETS_DIR` |
//! | [`Jail::indirection`] | `<PREFIX><KEY>_FILE=/path` |
//! | [`Jail::secrets_volume`], [`Jail::config_volume`] | a whole mount, `..data` symlinks included — see [`Layout`] |
//!
//! Deriving matters more than it looks. A test that sets `TEST_AUTH__JWT_SECRET_FILE` by hand
//! keeps passing after [`Terrace::file_suffix`](crate::Terrace::file_suffix) renames the
//! mechanism, while testing a variable the loader no longer reads;
//! `jail.indirection("auth.jwt_secret", …)` cannot.
//!
// The reloading half exists only when the supervisor does, and `broken_intra_doc_links` is
// denied — so this section is a gated `doc` attribute rather than a `//!` comment, the same way
// the crate documentation is. Interleaved with the `//!` blocks around it because rustdoc
// concatenates every inner attribute in source order, which puts it back where it reads.
#![cfg_attr(
    feature = "reload",
    doc = r#"
# Reloading

[`Rebuilds`] records what the supervisor built and waits for it — including the wait that is
supposed to time out, which is how "a failed reload leaves the running service alone" is
asserted — and [`ServiceError`] is the error type [`reload::run`](crate::reload::run) asks a
service for.
"#
)]
//! # Using it from another crate
//!
//! `testing` is a development dependency. It is deliberately not part of `full`, so that a
//! service asking for everything this crate does at runtime does not link a test harness:
//!
//! ```toml
//! [dependencies]
//! terrace-config = { git = "…", tag = "…" }
//!
//! [dev-dependencies]
//! terrace-config = { git = "…", tag = "…", features = ["testing"] }
//! ```

mod harness;
mod jail;
mod sandbox;
mod volume;

pub use harness::Harness;
pub use jail::Jail;
pub use sandbox::Sandbox;
pub use volume::{Layout, Volume};

#[cfg(feature = "reload")]
mod supervisor;

#[cfg(feature = "reload")]
pub use supervisor::{Rebuilds, ServiceError};
