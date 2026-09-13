//! The supervisor, driven against a real filesystem and the real loader.
//!
//! This file is where the two feature sets meet, which is the only place they are allowed to:
//! `terrace_config::Sources` implements `terrace_config::reload::Source`, and `run` is generic
//! over that trait rather than over the loader.
//!
//! Both tests have the same skeleton, because there are only two things a supervisor test can
//! assert: that a change rebuilt the runtime, and that some other change did not. The skeleton
//! itself lives in [`terrace_config::testing`] — [`Rebuilds`] records what the build closure was
//! handed, waits for a build that should come, and waits out one that should not.

#![cfg(all(feature = "loader", feature = "reload", feature = "testing"))]

use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use terrace_config::Terrace;
use terrace_config::reload::Debounce;
use terrace_config::testing::{Harness, Rebuilds, ServiceError};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize)]
struct TestConfig {
    database: Database,
}

#[derive(Debug, Deserialize)]
struct Database {
    url: SecretString,
}

fn harness() -> Harness {
    Harness::over(Terrace::new("TEST_"))
}

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

    harness().run(|jail| {
        jail.secret("database__url", "postgres://one/app")?;

        let boot = jail.load_watched::<TestConfig>()?;
        let loader = jail.terrace();
        let files = jail.sandbox();
        let rebuilds: Rebuilds = Rebuilds::new();

        jail.block_on(async {
            let shutdown = CancellationToken::new();

            let driver = rebuilds.clone();
            let stop = shutdown.clone();
            tokio::spawn(async move {
                driver.wait_for(1).await;

                files
                    .write("secrets/database__url", "postgres://two/app")
                    .expect("rotate");
                driver.wait_for(2).await;

                // `.` is refused as a key, so this is a reload that fails to *load* while the
                // directory it lives in is still perfectly readable.
                files.write("secrets/bad.key", "x").expect("break");
                driver.stays_at(2, debounce.0 * 6).await;

                stop.cancel();
            });

            terrace_config::reload::run(
                (boot.value, boot.sources),
                &shutdown,
                || {
                    loader
                        .load_watched::<TestConfig>()
                        .map(|loaded| (loaded.value, loaded.sources))
                        .map_err(ServiceError::from)
                },
                rebuilds
                    .serving(|config: &TestConfig| config.database.url.expose_secret().to_owned()),
            )
            .await
            .expect("the supervisor returns when shutdown is cancelled");
        });

        assert_eq!(
            rebuilds.seen(),
            ["postgres://one/app", "postgres://two/app"],
            "the rebuild must use the rotated value, exactly once"
        );
        Ok(())
    });
}

/// A reload that resolves to the values already running is a no-op. A `..data` swap that moved
/// no key would otherwise rebuild the pool and rebind the listener for nothing.
#[test]
fn a_change_that_resolves_to_the_same_values_does_not_rebuild() {
    let debounce = Debounce::default();

    harness().run(|jail| {
        jail.secret("database__url", "postgres://one/app")?;

        let boot = jail.load_watched::<TestConfig>()?;
        let loader = jail.terrace();
        let files = jail.sandbox();
        // The values are not compared here — only how often the runtime was built — so there is
        // nothing to record but the fact of it.
        let rebuilds: Rebuilds<()> = Rebuilds::new();

        jail.block_on(async {
            let shutdown = CancellationToken::new();

            let driver = rebuilds.clone();
            let stop = shutdown.clone();
            tokio::spawn(async move {
                driver.wait_for(1).await;

                // Rewritten with byte-identical contents, which is what a `ConfigMap` rollout
                // that changed nothing looks like on disk.
                files
                    .write("secrets/database__url", "postgres://one/app")
                    .expect("rewrite");
                driver.stays_at(1, debounce.0 * 6).await;

                stop.cancel();
            });

            terrace_config::reload::run(
                (boot.value, boot.sources),
                &shutdown,
                || {
                    loader
                        .load_watched::<TestConfig>()
                        .map(|loaded| (loaded.value, loaded.sources))
                        .map_err(ServiceError::from)
                },
                rebuilds.serving(|_: &TestConfig| ()),
            )
            .await
            .expect("the supervisor returns when shutdown is cancelled");
        });

        Ok(())
    });
}
