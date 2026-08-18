//! The sandboxed environment one test runs in.

use std::fmt::Display;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use super::sandbox::Sandbox;
use super::volume::Volume;
use crate::dialect::Dialect;
use crate::error::Error;
use crate::loaded::Loaded;
use crate::terrace::Terrace;

/// Where [`Jail::secret`] mounts its secrets directory.
const SECRETS_DIR: &str = "secrets";

/// Where [`Jail::fragment`] mounts its directory of TOML fragments.
const CONFIG_DIR: &str = "conf.d";

/// The file [`Jail::config`] writes.
const CONFIG_FILE: &str = "config.toml";

/// Where [`Jail::indirection`] writes the files its variables point at. Beside the secrets
/// directory rather than inside it: a `_FILE` variable names a path anywhere on the filesystem,
/// and one that happened to sit in the secrets directory would be supplying its key twice.
const INDIRECTION_DIR: &str = "indirect";

/// The environment and filesystem one test runs in.
///
/// Handed to the closure of [`Harness::run`](super::Harness::run). Everything it creates lives in
/// a temporary directory that is deleted when the test returns, and every variable it sets is
/// restored — including the ones the test never mentioned, because the harness clears the
/// environment on the way in.
///
/// The methods come in three layers, and a test can mix them freely:
///
/// - **The layers of the loader**: [`secret`](Self::secret), [`config`](Self::config),
///   [`fragment`](Self::fragment), [`indirection`](Self::indirection). Each writes a file *and*
///   points the variable that makes the loader read it, so a test says what it is arranging
///   rather than how the arrangement is spelled.
/// - **Whole mounts**: [`secrets_volume`](Self::secrets_volume),
///   [`config_volume`](Self::config_volume), [`volume`](Self::volume) — a
///   [`Volume`] builder for the shapes Kubernetes produces, `..data` symlinks included.
/// - **The raw sandbox**: [`env`](Self::env), [`write`](Self::write),
///   [`path`](Self::path), and [`sandbox`](Self::sandbox) for a handle that can leave the
///   closure.
///
/// Loading is on here too — [`load`](Self::load) and [`load_watched`](Self::load_watched) — so
/// that the loader under test is the one the harness was configured with and cannot drift from
/// the variables the harness just set.
pub struct Jail<'figment> {
    /// The environment and temporary directory, owned by `figment`.
    inner: &'figment mut figment::Jail,
    /// The same temporary directory, as a handle that outlives a borrow of `inner`.
    sandbox: Sandbox,
    /// The loader under test.
    terrace: Terrace,
    /// Its environment spelling, resolved once.
    dialect: Dialect,
}

impl<'figment> Jail<'figment> {
    /// Wrap a `figment::Jail` as the sandbox for `terrace`.
    pub(super) fn new(inner: &'figment mut figment::Jail, terrace: Terrace) -> Self {
        let sandbox = Sandbox::new(inner.directory().to_path_buf());
        let dialect = terrace.dialect();
        Self {
            inner,
            sandbox,
            terrace,
            dialect,
        }
    }

    // -----------------------------------------------------------------------------------------
    // The environment
    // -----------------------------------------------------------------------------------------

    /// Set an environment variable for the rest of the test.
    ///
    /// The value is anything that prints, so a path goes in as `path.display()`.
    pub fn env(&mut self, name: impl AsRef<str>, value: impl Display) -> &mut Self {
        self.inner.set_env(name, value);
        self
    }

    /// Set the variable that supplies `key`, in this loader's own spelling.
    ///
    /// `jail.env_key("auth.jwt_secret", "…")` sets `TEST_AUTH__JWT_SECRET` for a `TEST_` loader
    /// with the default separator, and follows [`Terrace::nesting_separator`] when it is not.
    /// A test that spells the variable out by hand keeps passing after the separator changes,
    /// while testing a name the loader no longer reads.
    pub fn env_key(&mut self, key: &str, value: impl Display) -> &mut Self {
        let name = self.dialect.env_spelling(key);
        self.env(name, value)
    }

    // -----------------------------------------------------------------------------------------
    // The sandbox
    // -----------------------------------------------------------------------------------------

    /// A handle to the sandbox directory that can be cloned and moved into another thread.
    ///
    /// The jail itself borrows the environment and cannot leave the closure. A test driving a
    /// supervisor needs to rotate a secret from a spawned task while the supervisor watches for
    /// it, and this is what it moves in.
    #[must_use]
    pub fn sandbox(&self) -> Sandbox {
        self.sandbox.clone()
    }

    /// An absolute path inside the sandbox, whether or not anything is there.
    ///
    /// See [`Sandbox::path`]. Naming a path that does not exist is the point of it: a mount an
    /// operator promised and did not make is a case the loader has to fail on.
    #[must_use]
    pub fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.sandbox.path(relative)
    }

    /// Write a file in the sandbox, creating parents, and return its absolute path.
    ///
    /// # Errors
    /// As [`Sandbox::write`].
    pub fn write(
        &self,
        relative: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> Result<PathBuf, Error> {
        self.sandbox.write(relative, contents)
    }

    /// Create a directory in the sandbox and return its absolute path.
    ///
    /// # Errors
    /// As [`Sandbox::create_dir`].
    pub fn create_dir(&self, relative: impl AsRef<Path>) -> Result<PathBuf, Error> {
        self.sandbox.create_dir(relative)
    }

    /// Create a symbolic link in the sandbox and return its absolute path.
    ///
    /// # Errors
    /// As [`Sandbox::symlink`], whose documentation also explains why this is Unix-only.
    #[cfg(unix)]
    pub fn symlink(
        &self,
        link: impl AsRef<Path>,
        target: impl AsRef<Path>,
    ) -> Result<PathBuf, Error> {
        self.sandbox.symlink(link, target)
    }

    // -----------------------------------------------------------------------------------------
    // The layers
    // -----------------------------------------------------------------------------------------

    /// Mount a secrets directory and put one key-named file in it.
    ///
    /// The name is taken **verbatim** — it is a mounted file name, `auth__jwt_secret`, and half
    /// of what these tests are about is what happens to a name that is not one.
    /// [`Self::secret_key`] derives it from a key path instead. Calling either repeatedly adds
    /// to the same directory; the variable naming that directory is set every time.
    ///
    /// # Errors
    /// [`Error::Source`] if the file cannot be written.
    pub fn secret(
        &mut self,
        name: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> Result<PathBuf, Error> {
        let dir = self.secrets_dir()?;
        self.sandbox.write(dir.join(name.as_ref()), contents)
    }

    /// Mount a secrets directory and put `key` in it, under the file name this loader spells
    /// that key with.
    ///
    /// `jail.secret_key("auth.jwt_secret", …)` writes `auth__jwt_secret` by default, and follows
    /// [`Terrace::nesting_separator`] when it is not the default. [`Self::secret`] takes the file
    /// name verbatim instead, which is what a test about a *name* — one nested with the wrong
    /// separator, one that is reserved — needs.
    ///
    /// # Errors
    /// [`Error::Source`] if the file cannot be written.
    pub fn secret_key(&mut self, key: &str, contents: impl AsRef<[u8]>) -> Result<PathBuf, Error> {
        let name = key.replace('.', self.dialect.separator());
        self.secret(name, contents)
    }

    /// Mount an empty secrets directory and return its absolute path.
    ///
    /// Worth calling on its own for the case the provider has to get right and no file can
    /// express: a `Secret` mounted with no keys in it yet.
    ///
    /// # Errors
    /// [`Error::Source`] if the directory cannot be created.
    pub fn secrets_dir(&mut self) -> Result<PathBuf, Error> {
        let dir = self.sandbox.create_dir(SECRETS_DIR)?;
        self.secrets_dir_at(&dir);
        Ok(dir)
    }

    /// Point the secrets-directory variable at `path`, creating nothing.
    ///
    /// For the mount that was promised and not made: a directory that is not there has to fail
    /// the boot, because booting on defaults instead is the outcome worth avoiding.
    pub fn secrets_dir_at(&mut self, path: impl AsRef<Path>) -> &mut Self {
        let name = self.terrace.secrets_dir_var_name();
        self.env(name, path.as_ref().display())
    }

    /// Write the TOML configuration file and point the configuration variable at it.
    ///
    /// Replaces whatever [`Self::fragment`] or [`Self::config_at`] pointed that variable at:
    /// the loader reads one configuration variable, so a test that arranges both is arranging
    /// only the last of them.
    ///
    /// # Errors
    /// [`Error::Source`] if the file cannot be written.
    pub fn config(&mut self, contents: impl AsRef<[u8]>) -> Result<PathBuf, Error> {
        let path = self.sandbox.write(CONFIG_FILE, contents)?;
        self.config_at(&path);
        Ok(path)
    }

    /// Write one fragment of a TOML configuration *directory*, and point the configuration
    /// variable at that directory.
    ///
    /// The mount this describes is a `ConfigMap` of several files, merged in name order —
    /// `10-base.toml` then `20-overrides.toml`. Calling it repeatedly adds to the same
    /// directory.
    ///
    /// # Errors
    /// [`Error::Source`] if the fragment cannot be written.
    pub fn fragment(
        &mut self,
        name: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> Result<PathBuf, Error> {
        let dir = self.sandbox.create_dir(CONFIG_DIR)?;
        self.config_at(&dir);
        self.sandbox.write(dir.join(name.as_ref()), contents)
    }

    /// Point the configuration variable at `path`, creating nothing.
    pub fn config_at(&mut self, path: impl AsRef<Path>) -> &mut Self {
        let name = self.terrace.config_var_name();
        self.env(name, path.as_ref().display())
    }

    /// Write a file holding one value and point a `_FILE` indirection variable at it.
    ///
    /// `key` is a figment key path — `auth.jwt_secret` — and the variable is derived from it the
    /// way the loader derives it: [`Terrace::file_suffix`] and
    /// [`Terrace::nesting_separator`] are both honoured, so a test does not restate a spelling
    /// that the loader alone decides.
    ///
    /// # Errors
    /// [`Error::Source`] if the file cannot be written.
    pub fn indirection(&mut self, key: &str, contents: impl AsRef<[u8]>) -> Result<PathBuf, Error> {
        // Named after the key, so a failure message points at the variable that produced it, and
        // so two indirections cannot land on one file.
        let name = key.replace(['.', '/', '\\'], "_");
        let path = self
            .sandbox
            .write(Path::new(INDIRECTION_DIR).join(name), contents)?;
        self.indirection_at(key, &path);
        Ok(path)
    }

    /// Point the `_FILE` indirection variable for `key` at `path`, creating nothing.
    ///
    /// For the other half of the contract: a path that cannot be read must fail the boot rather
    /// than be skipped, because a skipped secret is a service that comes up on a default.
    pub fn indirection_at(&mut self, key: &str, path: impl AsRef<Path>) -> &mut Self {
        let name = format!(
            "{}{}",
            self.dialect.env_spelling(key),
            self.dialect.indirection_suffix()
        );
        self.env(name, path.as_ref().display())
    }

    // -----------------------------------------------------------------------------------------
    // Whole mounts
    // -----------------------------------------------------------------------------------------

    /// A [`Volume`] builder at `dir`, wired to nothing.
    ///
    /// The general form of the two below: a directory that no variable points at, for a decoy a
    /// test needs the loader to *not* read.
    pub fn volume(&mut self, dir: impl Into<PathBuf>) -> Volume<'_, 'figment> {
        Volume::new(self, dir, None)
    }

    /// A [`Volume`] builder mounted where [`Self::secret`] mounts, wired to the
    /// secrets-directory variable.
    pub fn secrets_volume(&mut self) -> Volume<'_, 'figment> {
        let wire = self.terrace.secrets_dir_var_name();
        Volume::new(self, SECRETS_DIR, Some(wire))
    }

    /// A [`Volume`] builder mounted where [`Self::fragment`] mounts, wired to the configuration
    /// variable.
    pub fn config_volume(&mut self) -> Volume<'_, 'figment> {
        let wire = self.terrace.config_var_name();
        Volume::new(self, CONFIG_DIR, Some(wire))
    }

    // -----------------------------------------------------------------------------------------
    // Loading
    // -----------------------------------------------------------------------------------------

    /// The loader under test.
    ///
    /// A clone, so a single test can vary one knob — `jail.terrace().shadow_policy(…).load()` —
    /// without the variable names the jail has been setting moving underneath it.
    #[must_use]
    pub fn terrace(&self) -> Terrace {
        self.terrace.clone()
    }

    /// The environment spelling this loader reads.
    #[must_use]
    pub fn dialect(&self) -> &Dialect {
        &self.dialect
    }

    /// Load a typed config through the loader under test.
    ///
    /// # Errors
    /// As [`Terrace::load`].
    pub fn load<T: DeserializeOwned>(&self) -> Result<T, Error> {
        self.terrace.load()
    }

    /// Load a typed config and the sources it came from.
    ///
    /// # Errors
    /// As [`Terrace::load_watched`].
    pub fn load_watched<T: DeserializeOwned>(&self) -> Result<Loaded<T>, Error> {
        self.terrace.load_watched()
    }

    /// The assembled figment, for an assertion about a value the typed struct does not carry.
    ///
    /// # Errors
    /// As [`Terrace::figment`].
    pub fn figment(&self) -> Result<figment::Figment, Error> {
        self.terrace.figment()
    }

    /// Run `future` to completion on a runtime of this thread.
    ///
    /// The supervisor is driven from inside the jail rather than the other way round, because
    /// the jail owns the process environment: `#[tokio::test]` would put the environment under a
    /// runtime this test does not control, and every other test in the binary with it.
    ///
    /// # Panics
    /// If the runtime cannot be built, which on a test thread means the process is out of
    /// handles rather than that the code under test is wrong.
    #[cfg(feature = "reload")]
    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime for the test")
            .block_on(future)
    }
}

impl std::fmt::Debug for Jail<'_> {
    /// The sandbox and the loader. `figment::Jail` is not [`Debug`], and the environment it
    /// holds is the whole process's — printing that from a test harness is how a secret from
    /// some other variable ends up in a CI log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jail")
            .field("sandbox", &self.sandbox)
            .field("terrace", &self.terrace)
            .finish()
    }
}
