//! Driving [`reload::run`](crate::reload::run) from a test.
//!
//! A supervisor test asserts about *when* the runtime was built, and there are only two things
//! worth asserting: that a change rebuilt it with the new values, and that some other change did
//! not rebuild it at all. Both need the same two primitives — a record of what the build closure
//! was handed, and a way to wait for it without deadlocking a test that will never see it.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::reload::WatchError;

/// How long [`Rebuilds::wait_for`] waits before failing the test.
///
/// Long enough that a missed wake-up fails rather than hangs CI — a hung test is a timeout with
/// no output, twenty minutes later — and far longer than the debounce window a watcher normally
/// answers in, so a loaded runner does not fail a healthy supervisor.
const DEFAULT_PATIENCE: Duration = Duration::from_secs(15);

/// How often [`Rebuilds::wait_for`] looks again.
///
/// Polling rather than a notification: the assertion that nothing happened is half of what these
/// tests are for, and a channel that is never written cannot be waited on twice.
const POLL: Duration = Duration::from_millis(25);

/// What a recorded build returns: a future that serves until it is cancelled.
///
/// Boxed because a closure cannot name the type of the `async` block it returns, and this is the
/// return type of a closure. Not `Send`: [`reload::run`](crate::reload::run) awaits the runtime
/// on the caller's own task and never moves it.
type Serving = Pin<Box<dyn Future<Output = Result<(), ServiceError>>>>;

/// The error a supervised runtime returns.
///
/// [`reload::run`](crate::reload::run) is generic over the error a service already has, asking
/// only for `Display` and `From<WatchError>`. This is that type for a test that has no service:
/// it also converts from this crate's [`Error`](crate::Error), so a reload closure is
/// `jail.terrace().load_watched().map_err(ServiceError::from)` and nothing more.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServiceError {
    /// The filesystem watcher could not be installed.
    #[error("{0}")]
    Watch(#[from] WatchError),
    /// The configuration could not be loaded.
    #[error("{0}")]
    Config(#[from] crate::Error),
    /// The runtime itself failed, for a test that wants to assert on that.
    #[error("{0}")]
    Runtime(String),
}

/// What the runtime-building closure has been handed, in order.
///
/// One note per build — whatever the test wants to compare, usually the value that was supposed
/// to change — so that `seen()` is both the count and the evidence: a test that fails on the
/// count reports *what* was built rather than only how often.
///
/// Cloning shares the record, which is what lets the same one sit in the build closure and in the
/// task driving the test.
///
/// ```no_run
/// # use std::sync::Arc;
/// # use terrace_config::testing::{Harness, Rebuilds, ServiceError};
/// # #[derive(serde::Deserialize)] struct Config { url: String }
/// # fn main() { Harness::new("TEST_").run(|jail| {
/// let boot = jail.load_watched::<Config>()?;
/// let rebuilds: Rebuilds = Rebuilds::new();
///
/// jail.block_on(async {
///     let shutdown = tokio_util::sync::CancellationToken::new();
///     let driver = rebuilds.clone();
///     let cancel = shutdown.clone();
///     tokio::spawn(async move {
///         driver.wait_for(1).await;
///         cancel.cancel();
///     });
///
///     terrace_config::reload::run(
///         (boot.value, boot.sources),
///         &shutdown,
///         || Err(ServiceError::Runtime("no reload in this example".to_owned())),
///         rebuilds.serving(|config: &Config| config.url.clone()),
///     )
///     .await
/// })
/// .expect("the supervisor returns when shutdown is cancelled");
/// # Ok(()) }); }
/// ```
pub struct Rebuilds<T = String> {
    /// One entry per build, in order.
    seen: Arc<Mutex<Vec<T>>>,
    /// How long [`Self::wait_for`] waits.
    patience: Duration,
}

impl<T> Rebuilds<T> {
    /// An empty record, with the default patience.
    #[must_use]
    pub fn new() -> Self {
        Self {
            seen: Arc::new(Mutex::new(Vec::new())),
            patience: DEFAULT_PATIENCE,
        }
    }

    /// How long [`Self::wait_for`] waits before failing the test.
    ///
    /// Worth shortening only for a test that *expects* to wait it out, and even then the
    /// assertion that nothing happened is better written with [`Self::stays_at`], which does not
    /// have to fail to find out.
    #[must_use]
    pub fn patience(mut self, patience: Duration) -> Self {
        self.patience = patience;
        self
    }

    /// Record one build.
    pub fn record(&self, note: T) {
        self.locked().push(note);
    }

    /// How many builds have been recorded.
    #[must_use]
    pub fn count(&self) -> usize {
        self.locked().len()
    }

    /// Every build so far, in order.
    #[must_use]
    pub fn seen(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.locked().clone()
    }

    /// Wait until at least `builds` have been recorded.
    ///
    /// # Panics
    /// If they have not been recorded within [`Self::patience`], reporting how far the
    /// supervisor actually got. A test that hangs instead reports nothing at all.
    pub async fn wait_for(&self, builds: usize) {
        let deadline = tokio::time::Instant::now() + self.patience;
        while self.count() < builds {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the supervisor reached {} of {builds} builds in {:?}",
                self.count(),
                self.patience
            );
            tokio::time::sleep(POLL).await;
        }
    }

    /// Assert that the count is `builds` now and is still `builds` after `quiet`.
    ///
    /// The shape of every negative assertion a supervisor test makes: a reload that fails to
    /// load, or that resolves to the values already running, must leave the running service
    /// alone. `quiet` has to be a comfortable multiple of the
    /// [`Debounce`](crate::reload::Debounce) in force, since the rebuild being asserted against
    /// cannot happen before the window has elapsed.
    ///
    /// # Panics
    /// If the count is not `builds`, before or after the wait.
    pub async fn stays_at(&self, builds: usize, quiet: Duration) {
        assert_eq!(self.count(), builds, "before waiting {quiet:?}");
        tokio::time::sleep(quiet).await;
        assert_eq!(self.count(), builds, "after waiting {quiet:?}");
    }

    /// The record, which a poisoned lock would mean a previous assertion already failed inside.
    fn locked(&self) -> std::sync::MutexGuard<'_, Vec<T>> {
        self.seen
            .lock()
            .expect("the rebuild record is not poisoned")
    }
}

impl<T: Send + 'static> Rebuilds<T> {
    /// A `build` closure for [`reload::run`](crate::reload::run) that records `note(&config)`
    /// and then serves until it is cancelled.
    ///
    /// Serving until cancelled is the part worth having written once. A build closure that
    /// returns as soon as it has recorded looks right and ends the supervisor: `run` returns
    /// when the runtime returns of its own accord, so the test would pass its first assertion
    /// and then never see a reload.
    pub fn serving<C: 'static>(
        &self,
        note: impl Fn(&C) -> T + 'static,
    ) -> impl Fn(Arc<C>, CancellationToken) -> Serving {
        let rebuilds = self.clone();
        move |config: Arc<C>, token: CancellationToken| {
            rebuilds.record(note(&config));
            Box::pin(async move {
                token.cancelled().await;
                Ok(())
            })
        }
    }
}

impl<T> Clone for Rebuilds<T> {
    /// Shares the record rather than copying it, so the copy in the build closure and the copy in
    /// the task driving the test are one record.
    ///
    /// Derived, it would demand `T: Clone` for a clone that never touches a `T`.
    fn clone(&self) -> Self {
        Self {
            seen: Arc::clone(&self.seen),
            patience: self.patience,
        }
    }
}

impl<T> Default for Rebuilds<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Rebuilds<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rebuilds")
            .field("seen", &*self.locked())
            .field("patience", &self.patience)
            .finish()
    }
}
