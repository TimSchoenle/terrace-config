//! The supervisor, driven against a real filesystem and the real loader.
//!
//! This file is where the two feature sets meet, which is the only place they are allowed to:
//! `terrace_config::Sources` implements `terrace_config::reload::Source`, and `run` is generic
//! over that trait rather than over the loader.

#![cfg(all(feature = "loader", feature = "reload"))]
#![expect(
    clippy::result_large_err,
    reason = "figment::Jail::expect_with fixes the closure's error type to figment::Error"
)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use terrace_config::Terrace;
use terrace_config::reload::{Debounce, WatchError};

#[derive(Debug, Deserialize)]
struct TestConfig {
    database: Database,
}

#[derive(Debug, Deserialize)]
struct Database {
    url: SecretString,
}

/// The error a consuming service would already have. `From<WatchError>` is the only thing this
/// crate asks of it.
#[derive(Debug, thiserror::Error)]
enum ServiceError {
    #[error("{0}")]
    Watch(#[from] WatchError),
    #[error("configuration: {0}")]
    Config(String),
}

impl From<terrace_config::Error> for ServiceError {
    fn from(error: terrace_config::Error) -> Self {
        Self::Config(error.to_string())
    }
}

fn layers() -> Terrace {
    Terrace::new("TEST_")
}

/// Long enough that a missed wake-up fails rather than hangs CI; the watcher normally answers
/// within the debounce window.
const PATIENCE: Duration = Duration::from_secs(15);

/// The two behaviours the supervisor exists for, in one run: a rotated secret rebuilds the
/// runtime with the new value, and a reload that *fails to load* leaves the runtime that is
/// already serving completely alone.
///
/// The second half is the one worth pinning. The reload path runs the same code that at boot
/// only ever ran under a scheduler that would restart the container, so if a failed reload ever
/// propagated instead of being swallowed, a single bad file write would take down every healthy
/// pod at once — and nothing about the happy path would look different.
#[test]
fn a_rotated_secret_rebuilds_and_a_broken_reload_does_not() {
    let debounce = Debounce::default();

    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.create_dir("secrets")?;
        jail.create_file("secrets/database__url", "postgres://one/app")?;
        let dir = jail.directory().join("secrets");
        jail.set_env("TEST_SECRETS_DIR", dir.display());

        let boot = layers()
            .load_watched::<TestConfig>()
            .map_err(|e| e.to_string())
            .unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let shutdown = tokio_util::sync::CancellationToken::new();

            let recorded = Arc::clone(&seen);
            let driver_seen = Arc::clone(&seen);
            let driver_shutdown = shutdown.clone();
            let driver_dir = dir.clone();

            tokio::spawn(async move {
                let count = |n: usize| {
                    let seen = Arc::clone(&driver_seen);
                    async move {
                        let deadline = tokio::time::Instant::now() + PATIENCE;
                        while seen.lock().expect("not poisoned").len() < n {
                            assert!(
                                tokio::time::Instant::now() < deadline,
                                "the supervisor never reached {n} builds"
                            );
                            tokio::time::sleep(Duration::from_millis(25)).await;
                        }
                    }
                };

                count(1).await;
                std::fs::write(driver_dir.join("database__url"), "postgres://two/app")
                    .expect("rotate");
                count(2).await;

                // `.` is refused as a key, so this is a reload that fails to *load* while the
                // directory it lives in is still perfectly readable.
                std::fs::write(driver_dir.join("bad.key"), "x").expect("break");
                tokio::time::sleep(debounce.0 * 6).await;
                assert_eq!(
                    driver_seen.lock().expect("not poisoned").len(),
                    2,
                    "a failed reload must not rebuild the running service"
                );

                driver_shutdown.cancel();
            });

            terrace_config::reload::run(
                (boot.value, boot.sources),
                &shutdown,
                || {
                    layers()
                        .load_watched::<TestConfig>()
                        .map(|loaded| (loaded.value, loaded.sources))
                        .map_err(ServiceError::from)
                },
                move |cfg: Arc<TestConfig>, token: tokio_util::sync::CancellationToken| {
                    recorded
                        .lock()
                        .expect("not poisoned")
                        .push(cfg.database.url.expose_secret().to_owned());
                    async move {
                        token.cancelled().await;
                        Ok::<(), ServiceError>(())
                    }
                },
            )
            .await
            .expect("the supervisor returns when shutdown is cancelled");

            let seen = seen.lock().expect("not poisoned").clone();
            assert_eq!(
                seen,
                ["postgres://one/app", "postgres://two/app"],
                "the rebuild must use the rotated value, exactly once"
            );
        });

        Ok(())
    });
}

/// A reload that resolves to the values already running is a no-op. A `..data` swap that moved
/// no key would otherwise rebuild the pool and rebind the listener for nothing.
#[test]
fn a_change_that_resolves_to_the_same_values_does_not_rebuild() {
    let debounce = Debounce::default();

    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.create_dir("secrets")?;
        jail.create_file("secrets/database__url", "postgres://one/app")?;
        let dir = jail.directory().join("secrets");
        jail.set_env("TEST_SECRETS_DIR", dir.display());

        let boot = layers()
            .load_watched::<TestConfig>()
            .map_err(|e| e.to_string())
            .unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async move {
            let builds = Arc::new(Mutex::new(0_usize));
            let shutdown = tokio_util::sync::CancellationToken::new();

            let counted = Arc::clone(&builds);
            let driver_builds = Arc::clone(&builds);
            let driver_shutdown = shutdown.clone();
            let driver_dir = dir.clone();

            tokio::spawn(async move {
                let deadline = tokio::time::Instant::now() + PATIENCE;
                while *driver_builds.lock().expect("not poisoned") < 1 {
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "the supervisor never built once"
                    );
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }

                // Rewritten with byte-identical contents, which is what a `ConfigMap` rollout
                // that changed nothing looks like on disk.
                std::fs::write(driver_dir.join("database__url"), "postgres://one/app")
                    .expect("rewrite");
                tokio::time::sleep(debounce.0 * 6).await;
                assert_eq!(
                    *driver_builds.lock().expect("not poisoned"),
                    1,
                    "an identical rewrite must not rebuild the running service"
                );

                driver_shutdown.cancel();
            });

            terrace_config::reload::run(
                (boot.value, boot.sources),
                &shutdown,
                || {
                    layers()
                        .load_watched::<TestConfig>()
                        .map(|loaded| (loaded.value, loaded.sources))
                        .map_err(ServiceError::from)
                },
                move |_cfg: Arc<TestConfig>, token: tokio_util::sync::CancellationToken| {
                    *counted.lock().expect("not poisoned") += 1;
                    async move {
                        token.cancelled().await;
                        Ok::<(), ServiceError>(())
                    }
                },
            )
            .await
            .expect("the supervisor returns when shutdown is cancelled");
        });

        Ok(())
    });
}
