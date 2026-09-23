//! The supervisor over [`Terrace::reloader`]: a restart-class key keeps its boot value.
//!
//! The contract publishes some keys as `restart`, and a chart acting on that leaves them out of
//! the digest that rolls its pods for a `live` change. That is only true if the running process
//! agrees — if a rebuild triggered by a `live` key did not quietly apply a `restart` key changed
//! in the same mount. These tests hold the runtime to what the document says.

#![cfg(all(
    feature = "loader",
    feature = "reload",
    feature = "schema",
    feature = "testing"
))]

use serde::Deserialize;
use terrace_config::Terrace;
use terrace_config::reload::Debounce;
use terrace_config::schema::{Describe, Reload, ReloadSupport};
use terrace_config::testing::{Harness, Rebuilds, ServiceError};
use tokio_util::sync::CancellationToken;

/// One key a rebuild applies, one it does not.
#[derive(Debug, Deserialize, Describe)]
#[config(reload = "live")]
struct TestConfig {
    /// Where the service forwards to, which the rebuilt runtime reads.
    upstream: String,
    /// The log filter, which a subscriber installed before the supervisor reads.
    #[config(reload = "restart")]
    level: String,
}

fn loader() -> Terrace {
    Terrace::new("TEST_").reloads(ReloadSupport::rebuild())
}

/// A restart key changed on its own rebuilds nothing; a live key changed afterwards rebuilds with
/// the live value and the restart key still at its boot value.
#[test]
fn a_restart_key_keeps_its_boot_value_across_a_rebuild() {
    let debounce = Debounce::default();

    Harness::over(loader()).run(|jail| {
        jail.secret("upstream", "http://one")?;
        jail.secret("level", "info")?;

        let (boot, reloader) = jail.terrace().reloader::<TestConfig>()?;
        assert!(boot.sources.pending_restart().is_empty());
        let files = jail.sandbox();
        let rebuilds: Rebuilds = Rebuilds::new();

        jail.block_on(async {
            let shutdown = CancellationToken::new();

            let driver = rebuilds.clone();
            let stop = shutdown.clone();
            tokio::spawn(async move {
                driver.wait_for(1).await;

                files
                    .write("secrets/level", "debug")
                    .expect("change the level");
                driver.stays_at(1, debounce.0 * 6).await;

                files
                    .write("secrets/upstream", "http://two")
                    .expect("change the upstream");
                driver.wait_for(2).await;

                stop.cancel();
            });

            terrace_config::reload::run(
                boot.into(),
                &shutdown,
                || {
                    reloader
                        .reload()
                        .map(Into::into)
                        .map_err(ServiceError::from)
                },
                rebuilds
                    .serving(|config: &TestConfig| format!("{} {}", config.upstream, config.level)),
            )
            .await
            .expect("the supervisor returns when shutdown is cancelled");
        });

        assert_eq!(
            rebuilds.seen(),
            ["http://one info", "http://two info"],
            "the rebuild applies the live key and keeps the restart key at its boot value"
        );
        Ok(())
    });
}

/// The reload itself reports what it held back, so a service can surface it.
#[test]
fn a_held_back_change_is_reported_as_pending() {
    Harness::over(loader()).run(|jail| {
        jail.secret("upstream", "http://one")?;
        jail.secret("level", "info")?;

        let (_, reloader) = jail.terrace().reloader::<TestConfig>()?;
        jail.secret("level", "debug")?;

        let held = reloader.reload()?;
        assert_eq!(held.value.level, "info");
        assert_eq!(held.sources.pending_restart(), ["level"]);

        jail.secret("level", "info")?;
        assert!(reloader.reload()?.sources.pending_restart().is_empty());
        Ok(())
    });
}

/// A binary that did not declare a rebuild has no business pinning, and one that did has no
/// business rebuilding with every key.
#[test]
fn each_loader_refuses_the_other_declaration() {
    Harness::over(Terrace::new("TEST_")).run(|jail| {
        jail.secret("upstream", "http://one")?;
        jail.secret("level", "info")?;
        assert!(jail.terrace().reloader::<TestConfig>().is_err());
        Ok(())
    });

    Harness::over(loader()).run(|jail| {
        jail.secret("upstream", "http://one")?;
        jail.secret("level", "info")?;
        assert!(jail.load_watched::<TestConfig>().is_err());
        Ok(())
    });
}

/// The published classes are the ones the runtime pins by.
#[test]
fn the_schema_publishes_what_the_reloader_pins() {
    let schema = loader().schema::<TestConfig>();
    let classes: Vec<_> = schema
        .keys
        .iter()
        .map(|key| (key.path.as_str(), key.reload))
        .collect();
    assert_eq!(
        classes,
        [
            ("upstream", Some(Reload::Live)),
            ("level", Some(Reload::Restart))
        ]
    );
    assert_eq!(schema.restart_paths(), ["level"]);
}
