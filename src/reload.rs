//! Rebuilding a running service when the files its configuration came from change.
//!
//! A Kubernetes `Secret` or `ConfigMap` mounted as a volume is updated in place by the kubelet:
//! a new timestamped directory is written and `..data` is renamed over the old one. That is the
//! only way a long-lived process learns a credential was rotated — environment variables are
//! fixed for the life of a process — so a service that takes its secrets from files can pick up
//! a rotation without being restarted.
//!
//! [`run`] takes the closure that builds the whole runtime and re-runs it. Everything the
//! closure builds is rebuilt: the connection pool, the application state, the router, the
//! listener, the background tasks. That is deliberate — the alternative, hot-swapping
//! individual fields behind shared handles, means every consumer has to be correct against a
//! value that changes underneath it, and the failure when one is not is a service running half
//! on the old configuration.
//!
//! # What is *not* rebuilt
//! Process-global installations that happen before [`run`] is reached and cannot be redone: a
//! `tracing` subscriber, a metrics recorder. Changing the configuration that drives those still
//! needs a restart.
//!
//! # Failure posture
//! A reload that cannot be loaded, or that fails to build, leaves the running service exactly
//! as it was. This matters more than it sounds: the reload path runs the same code that at boot
//! only ever ran under a scheduler that would restart the container, so a bad file write must
//! not be able to take down a pod that is currently healthy.
//!
//! # Independence
//! **Nothing in this module depends on the `loader` feature, and nothing in it should.** The
//! supervisor is useful to anyone with a `Fn(Arc<C>, CancellationToken) -> Future` and a way to
//! detect change, regardless of how they load configuration; coupling it to figment would
//! shrink its audience for no benefit. [`Source`] is the seam, and `terrace_config::Sources`
//! implements it when both features are on — named in prose rather than linked, so that this
//! module's documentation resolves without the `loader` feature.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// The channel depth between the watcher thread and the supervisor.
///
/// One, because the signal carries no information: any pending notification means "re-read",
/// and a burst of fifty is the same instruction as one. A full channel is therefore dropped
/// rather than queued.
const SIGNAL_DEPTH: usize = 1;

/// How long the filesystem must be quiet before a change is acted on.
///
/// One logical Kubernetes volume update fires several events — the new directory is created,
/// files are written, `..data` is renamed — and rebuilding on the first of them would read a
/// half-written mount. The kubelet's own sync period is on the order of a minute, so half a
/// second of extra latency costs nothing and removes the whole class of torn reads.
///
/// A parameter rather than a constant because a crate cannot hardcode one deployment's kubelet
/// sync period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Debounce(pub Duration);

impl Default for Debounce {
    fn default() -> Self {
        Self(Duration::from_millis(500))
    }
}

/// The filesystem watcher could not be installed.
///
/// Deliberately a plain string rather than a `notify::Error`: it keeps `notify` out of every
/// consumer's error enum, and there is nothing a caller can do with the structured form that it
/// cannot do with the message.
#[derive(Debug, Clone, thiserror::Error)]
#[error("configuration watch error: {0}")]
pub struct WatchError(pub String);

/// Where a configuration came from, and whether it has since changed.
///
/// A trait rather than a `F: PartialEq` fingerprint type parameter. The fingerprint the
/// `loader` feature uses has to be a `figment::value::Value`, because comparing the typed
/// config struct is impossible — config structs hold secret types that deliberately have no
/// `PartialEq`. A type parameter would work and would keep figment out of this module, but it
/// leaks the mechanism into every caller's signature. The trait hides it in one impl on the one
/// type that has it.
pub trait Source: Sized {
    /// Directories to watch for changes.
    fn watch_paths(&self) -> &[PathBuf];

    /// Whether `self` resolves to different values than `previous`.
    fn differs_from(&self, previous: &Self) -> bool;
}

/// Run a service, rebuilding it whenever its configuration files change.
///
/// `build` receives the current configuration and a token that is cancelled when the runtime
/// must stop — either because the process is shutting down or because a rebuild is due. It
/// should return once it has stopped: the replacement is not built until it does, so the old
/// listener has released the address before the new one binds it.
///
/// `reload` re-reads the configuration from scratch and is called once per debounced change. A
/// reload that cannot be loaded, or that resolves to the values already running, leaves the
/// running service exactly as it is.
///
/// Returns when the runtime returns of its own accord, which for a serving service means the
/// shutdown signal has been handled and in-flight requests have drained.
///
/// Uses the default [`Debounce`]; see [`run_with`] to choose one.
///
/// # Errors
/// Returns `E::from(WatchError)` if the filesystem watcher cannot be installed, or whatever the
/// runtime itself returned. A *reload* failure is never returned — it is logged and the running
/// configuration is kept.
pub async fn run<C, S, R, F, Fut, E>(
    boot: (C, S),
    shutdown: &CancellationToken,
    reload: R,
    build: F,
) -> Result<(), E>
where
    S: Source,
    R: Fn() -> Result<(C, S), E>,
    F: Fn(Arc<C>, CancellationToken) -> Fut,
    Fut: Future<Output = Result<(), E>>,
    E: std::fmt::Display + From<WatchError>,
{
    run_with(boot, shutdown, reload, build, Debounce::default()).await
}

/// [`run`], with an explicit debounce window.
///
/// # Errors
/// As [`run`].
pub async fn run_with<C, S, R, F, Fut, E>(
    boot: (C, S),
    shutdown: &CancellationToken,
    reload: R,
    build: F,
    debounce: Debounce,
) -> Result<(), E>
where
    S: Source,
    R: Fn() -> Result<(C, S), E>,
    F: Fn(Arc<C>, CancellationToken) -> Fut,
    Fut: Future<Output = Result<(), E>>,
    E: std::fmt::Display + From<WatchError>,
{
    let (value, mut sources) = boot;
    let mut config = Arc::new(value);
    let mut changes = Watch::install(sources.watch_paths(), debounce).map_err(E::from)?;

    let mut generation = shutdown.child_token();
    let running = build(Arc::clone(&config), generation.clone());
    tokio::pin!(running);

    loop {
        tokio::select! {
            outcome = &mut running => return outcome,
            () = changes.changed() => {
                let Some((next, next_sources)) = reread(&reload, &sources) else { continue };

                tracing::info!("configuration changed; rebuilding the service");
                generation.cancel();
                // Driven to completion rather than dropped: dropping the future would sever
                // in-flight requests and leave the listener's address in TIME_WAIT while the
                // replacement tries to bind it.
                if let Err(e) = (&mut running).await {
                    tracing::warn!(error = %e, "the previous runtime stopped with an error");
                }

                sources = next_sources;
                config = Arc::new(next);
                generation = shutdown.child_token();
                running.set(build(Arc::clone(&config), generation.clone()));
            }
        }
    }
}

/// Re-read the configuration, returning it only if it resolves to different values.
///
/// `None` covers both "nothing actually changed" and "the reload failed": neither is a reason
/// to touch a service that is currently working, and both are the common case — a `..data` swap
/// that moved no key, or a half-written mount caught between events.
fn reread<C, S, R, E>(reload: &R, current: &S) -> Option<(C, S)>
where
    S: Source,
    R: Fn() -> Result<(C, S), E>,
    E: std::fmt::Display,
{
    match reload() {
        Err(e) => {
            tracing::error!(
                error = %e,
                "configuration reload failed; keeping the running configuration"
            );
            None
        }
        Ok((_, sources)) if !sources.differs_from(current) => {
            tracing::debug!("configuration files changed but resolved to the same values");
            None
        }
        Ok(next) => Some(next),
    }
}

/// The filesystem watcher and the signal it feeds.
///
/// Holds the debouncer itself: dropping it stops the watch, so it has to outlive the supervisor
/// loop rather than the call that installed it.
struct Watch {
    /// `None` when there is nothing to watch — no secrets directory, no indirection target —
    /// in which case [`Self::changed`] never resolves and the service simply runs.
    signals: Option<mpsc::Receiver<()>>,
    /// Kept alive for its `Drop`; never read.
    _debouncer: Option<Debouncer<notify::RecommendedWatcher, RecommendedCache>>,
}

impl Watch {
    /// Install a debounced watch over every directory that exists.
    ///
    /// # Errors
    /// Returns [`WatchError`] if the platform watcher cannot be created or a directory cannot
    /// be watched.
    fn install(paths: &[PathBuf], debounce: Debounce) -> Result<Self, WatchError> {
        let watchable: Vec<&Path> = paths
            .iter()
            .map(PathBuf::as_path)
            // A path that does not exist cannot be watched, and is not an error: a service with
            // no secrets directory is the normal development case.
            .filter(|p| p.is_dir())
            .collect();
        if watchable.is_empty() {
            tracing::debug!("no configuration directories to watch; reload is inactive");
            return Ok(Self {
                signals: None,
                _debouncer: None,
            });
        }

        let (tx, rx) = mpsc::channel(SIGNAL_DEPTH);
        // `notify-debouncer-full` owns the timer, the event cache and the burst collapsing that
        // this module used to hand-roll as a sleep plus a `try_recv` drain. What arrives here is
        // already one notification per quiet period.
        let mut debouncer = new_debouncer(debounce.0, None, move |result: DebounceEventResult| {
            // The callback runs on the debouncer's own thread. `try_send` rather than a
            // blocking send: a full channel already means "re-read pending", so dropping this
            // one loses nothing and never parks that thread.
            if result.is_ok() {
                let _ = tx.try_send(());
            }
        })
        .map_err(|e| WatchError(e.to_string()))?;

        for path in watchable {
            // Non-recursive: a `Secret` volume is flat, and recursing into the timestamped
            // `..data` target would double every event for no extra coverage.
            debouncer
                .watch(path, RecursiveMode::NonRecursive)
                .map_err(|e| WatchError(format!("watching {}: {e}", path.display())))?;
            tracing::debug!(path = %path.display(), "watching for configuration changes");
        }

        Ok(Self {
            signals: Some(rx),
            _debouncer: Some(debouncer),
        })
    }

    /// Resolve once the watched files have changed *and* gone quiet again.
    async fn changed(&mut self) {
        let Some(signals) = self.signals.as_mut() else {
            // Nothing to watch: never resolve, so the caller's `select!` reduces to just
            // running the service.
            std::future::pending::<()>().await;
            return;
        };

        if signals.recv().await.is_none() {
            // The debouncer is gone (its thread died). Reload is over; the service keeps
            // running.
            self.signals = None;
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Debounce, Watch};
    use std::time::Duration;

    /// A watch over a directory fires on a file written into it, and collapses the burst that
    /// one write produces into a single wake-up.
    #[tokio::test]
    async fn a_write_into_a_watched_directory_wakes_the_supervisor() {
        let dir = std::env::temp_dir().join(format!("terrace-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let debounce = Debounce::default();
        let mut watch =
            Watch::install(std::slice::from_ref(&dir), debounce).expect("watch installs");

        std::fs::write(dir.join("auth__jwt_secret"), "rotated").expect("write");

        tokio::time::timeout(debounce.0 + Duration::from_secs(5), watch.changed())
            .await
            .expect("the write must wake the watcher");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// With nothing to watch, `changed()` must never resolve — otherwise the supervisor's
    /// `select!` would spin, rebuilding the service as fast as it can construct it.
    #[tokio::test]
    async fn nothing_to_watch_never_wakes() {
        let mut watch =
            Watch::install(&[], Debounce::default()).expect("an empty watch is not an error");
        let woke = tokio::time::timeout(Duration::from_millis(200), watch.changed()).await;
        assert!(woke.is_err(), "an empty watch must never resolve");
    }
}
