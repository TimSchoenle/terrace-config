//! The loader under test, and the sandbox each of its tests runs in.

use super::jail::Jail;
use crate::error::Error;
use crate::terrace::Terrace;

/// One loader, and the sandbox its tests run in.
///
/// Built once per test module and used by every test in it, so that the arrangement each test
/// writes and the loader it exercises cannot drift apart:
///
/// ```
/// use terrace_config::{Terrace, testing::Harness};
/// # #[derive(serde::Deserialize)] struct Config { auth: Auth }
/// # #[derive(serde::Deserialize)] struct Auth { jwt_secret: String }
///
/// fn harness() -> Harness {
///     Harness::over(Terrace::new("TEST_").reserve("TEST_PROFILE"))
/// }
///
/// harness().run(|jail| {
///     jail.secret("auth__jwt_secret", "s3cret\n")?;
///
///     let config: Config = jail.load()?;
///     assert_eq!(config.auth.jwt_secret, "s3cret");
///     Ok(())
/// });
/// ```
///
/// The closure returns this crate's own [`Error`], which is the point of the type: `?` works on
/// every arrangement *and* on the load being tested, and a test no longer carries an
/// `#[expect(clippy::result_large_err, …)]` for a `figment::Error` it never names.
#[derive(Debug, Clone)]
pub struct Harness {
    /// The loader every jail hands out.
    terrace: Terrace,
    /// Whether to remove the process's environment on the way in.
    clear_env: bool,
}

impl Harness {
    /// A harness over the loader `Terrace::new(prefix)` builds.
    #[must_use]
    pub fn new(prefix: impl Into<String>) -> Self {
        Self::over(Terrace::new(prefix))
    }

    /// A harness over a loader that has already been configured — reserved keys, a renamed
    /// variable, a shadow policy.
    ///
    /// The loader given here is the one [`Jail::load`] uses and the one every
    /// variable-setting method on [`Jail`] derives its names from. That is what makes
    /// [`Jail::indirection`] correct after a [`Terrace::file_suffix`] call, where a test
    /// spelling the variable out by hand would keep passing while testing a name the loader no
    /// longer reads.
    #[must_use]
    pub fn over(terrace: Terrace) -> Self {
        Self {
            terrace,
            clear_env: true,
        }
    }

    /// Keep the process's environment instead of clearing it.
    ///
    /// By default a jail starts from an empty environment. Most of what a loader test asserts is
    /// about what the environment does *not* contain — a key that must come from the mounted
    /// file, a variable that must not shadow it — and a developer with a real value exported
    /// would otherwise decide the outcome, on their machine only.
    ///
    /// Inheriting is for the test that needs something ambient: a `PATH` for a process it
    /// spawns, a `HOME` for a library that reads one. It is a real risk, taken deliberately, and
    /// there is no way to take it by halves: nothing can remove a single variable without
    /// `unsafe`, which this crate forbids.
    #[must_use]
    pub fn inherit_env(mut self) -> Self {
        self.clear_env = false;
        self
    }

    /// The loader under test.
    #[must_use]
    pub fn terrace(&self) -> &Terrace {
        &self.terrace
    }

    /// Run `test` in a fresh sandbox, and panic if it returns an error.
    ///
    /// The shape almost every test wants: an arrangement that fails to arrange is a broken test
    /// rather than a finding, and a panic is how a test reports that.
    ///
    /// # Panics
    /// If `test` returns [`Err`], with the error's message. A panic inside `test` — an
    /// `assert!` — propagates unchanged, and the environment and temporary directory are
    /// restored either way.
    #[track_caller]
    pub fn run<T>(&self, test: impl FnOnce(&mut Jail<'_>) -> Result<T, Error>) -> T {
        match self.try_run(test) {
            Ok(value) => value,
            Err(error) => panic!("the sandboxed test failed: {error}"),
        }
    }

    /// Run `test` in a fresh sandbox and return what it returned.
    ///
    /// For a caller that has something to do with the failure: a fuzz oracle, where a jail that
    /// could not be set up says nothing about the code under test, or a test asserting that a
    /// whole arrangement is refused.
    ///
    /// Sandboxes are process-wide and serialised against each other — the environment is a
    /// global — so two tests calling this never run at once, however many threads the test
    /// harness uses.
    ///
    /// # Errors
    /// Whatever `test` returns, or [`Error::Source`] if the sandbox itself could not be created.
    ///
    /// # Panics
    /// A panic inside `test` propagates unchanged.
    // `figment::Jail::try_with` fixes its closure's error type to the large `figment::Error`.
    // This is the one place in the workspace that closure is written, which is the point: the
    // expectation lives here instead of at the top of every test file in every consuming
    // project.
    #[expect(
        clippy::result_large_err,
        reason = "figment::Jail::try_with fixes the closure's error type to figment::Error"
    )]
    #[track_caller]
    pub fn try_run<T>(
        &self,
        test: impl FnOnce(&mut Jail<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut outcome = None;

        figment::Jail::try_with(|jail| {
            if self.clear_env {
                jail.clear_env();
            }
            let mut sandboxed = Jail::new(jail, self.terrace.clone());
            outcome = Some(test(&mut sandboxed));
            Ok(())
        })
        .map_err(|error| {
            Error::source(format!(
                "the test harness could not create a sandbox: {error}"
            ))
        })?;

        // `try_with` returned `Ok`, so the closure above ran to completion and left its result
        // here. A panic inside it would have unwound past this line rather than reaching it.
        outcome.expect("the sandboxed test ran")
    }
}
