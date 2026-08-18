//! The temporary directory one test runs in.

use std::path::{Component, Path, PathBuf};

use crate::error::Error;

/// A handle to the temporary directory one test runs in.
///
/// Every path a test writes is relative to this directory, and the whole tree is deleted when
/// the test returns. Nothing here reads or writes an environment variable — that is
/// [`Jail`](super::Jail)'s half — which is what lets this type be `Clone` and `'static` where
/// `Jail` is neither: a task driving a test from the outside, rotating a mounted secret while a
/// supervisor watches for it, needs a handle it can move into `tokio::spawn`.
///
/// ```no_run
/// # fn main() -> Result<(), terrace_config::Error> {
/// # let harness = terrace_config::testing::Harness::new("TEST_");
/// harness.try_run(|jail| {
///     let files = jail.sandbox();
///     std::thread::spawn(move || files.write("secrets/database__url", "postgres://two/app"));
///     Ok(())
/// })
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct Sandbox {
    /// The temporary directory, already canonicalised by `figment::Jail`.
    root: PathBuf,
}

impl Sandbox {
    /// A handle to `root`.
    pub(super) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The sandbox directory itself. It is also the process's working directory for the
    /// duration of the test, so a loader left on its default `config.toml` reads from here.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// An absolute path inside the sandbox.
    ///
    /// Pure path arithmetic: it touches no filesystem and checks nothing, so it answers for a
    /// file that does not exist yet — which is most of what a test needs it for, naming the
    /// path it is about to point a variable at. The escape check belongs to the operations that
    /// actually create something.
    #[must_use]
    pub fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }

    /// Create a directory and every missing parent, and return its absolute path.
    ///
    /// # Errors
    /// [`Error::Source`] if `relative` leaves the sandbox, or if the directory cannot be
    /// created.
    pub fn create_dir(&self, relative: impl AsRef<Path>) -> Result<PathBuf, Error> {
        let path = self.resolve(relative.as_ref())?;
        std::fs::create_dir_all(&path)
            .map_err(|e| failed("creating the directory", &path, &e.to_string()))?;
        Ok(path)
    }

    /// Write a file, creating every missing parent directory, and return its absolute path.
    ///
    /// The contents are written verbatim — no trailing newline is added, and none is removed.
    /// Both halves matter to this crate: the secrets provider strips a trailing newline because
    /// every editor adds one, and it must not strip a trailing space, so a test has to be able
    /// to write either.
    ///
    /// # Errors
    /// [`Error::Source`] if `relative` leaves the sandbox, or if the file cannot be written.
    pub fn write(
        &self,
        relative: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> Result<PathBuf, Error> {
        let path = self.resolve(relative.as_ref())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| failed("creating the directory", parent, &e.to_string()))?;
        }
        std::fs::write(&path, contents).map_err(|e| failed("writing", &path, &e.to_string()))?;
        Ok(path)
    }

    /// Remove a file, and return its absolute path.
    ///
    /// Removing something that is not there is an error rather than a no-op: a test that
    /// deletes the wrong path would otherwise assert against a mount it never changed.
    ///
    /// # Errors
    /// [`Error::Source`] if `relative` leaves the sandbox, or if the file cannot be removed.
    pub fn remove(&self, relative: impl AsRef<Path>) -> Result<PathBuf, Error> {
        let path = self.resolve(relative.as_ref())?;
        std::fs::remove_file(&path).map_err(|e| failed("removing", &path, &e.to_string()))?;
        Ok(path)
    }

    /// Create a symbolic link at `link`, pointing at `target`, and return the link's absolute
    /// path.
    ///
    /// **`target` is written into the link verbatim**, and is not resolved against the sandbox:
    /// a projected Kubernetes volume links each key to the *relative* path `..data/<key>`, and
    /// rewriting that as an absolute path would test a mount that never exists in a cluster.
    ///
    /// Unix only, which is the honest signature rather than a limitation of this crate. Windows
    /// needs either developer mode or a privilege for `std::os::windows::fs::symlink_file`,
    /// so a portable version of this would silently create regular files on the one platform
    /// where the distinction is invisible — and the distinction is the whole test: reading a
    /// projected volume through `DirEntry::metadata()`, which does not follow symlinks, reports
    /// every real key as "not a file" and yields a silently empty layer.
    ///
    /// A test that calls this therefore carries `#[cfg(unix)]` itself. See
    /// [`Layout::Symlinked`](super::Layout::Symlinked) for the whole-volume version.
    ///
    /// # Errors
    /// [`Error::Source`] if `link` leaves the sandbox, or if the link cannot be created.
    #[cfg(unix)]
    pub fn symlink(
        &self,
        link: impl AsRef<Path>,
        target: impl AsRef<Path>,
    ) -> Result<PathBuf, Error> {
        let link = self.resolve(link.as_ref())?;
        let target = target.as_ref();
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| failed("creating the directory", parent, &e.to_string()))?;
        }
        std::os::unix::fs::symlink(target, &link).map_err(|e| {
            failed(
                "symlinking",
                &link,
                &format!("to {}: {e}", target.display()),
            )
        })?;
        Ok(link)
    }

    /// `relative` as an absolute path inside the sandbox, refusing anything that leaves it.
    ///
    /// Lexical rather than [`Path::canonicalize`], because the path being resolved is usually
    /// one that does not exist yet. `..` is refused outright rather than folded away: a test
    /// writing outside its sandbox leaves a file behind that the next run inherits, and there is
    /// no reason to spell a path inside one that way.
    ///
    /// An absolute path that is already inside the sandbox is accepted unchanged. Every method
    /// here hands one back, and threading one into the next call — `jail.path("conf.d")`, then a
    /// file in it — should not mean making it relative again by hand.
    fn resolve(&self, relative: &Path) -> Result<PathBuf, Error> {
        let relative = relative.strip_prefix(&self.root).unwrap_or(relative);

        let mut path = self.root.clone();
        for component in relative.components() {
            match component {
                Component::Normal(part) => path.push(part),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(Error::source(format!(
                        "`{}` is not a path inside the test sandbox: every path a test writes \
                         is relative to it, with no `..` and no root, so that the tree can be \
                         deleted whole when the test returns.",
                        relative.display()
                    )));
                }
            }
        }
        Ok(path)
    }
}

/// One failed filesystem operation, worded the same way whichever operation it was.
///
/// A setup failure is not a finding about the code under test, so the message says plainly that
/// it is the harness that could not do its job, and names the path.
fn failed(operation: &str, path: &Path, cause: &str) -> Error {
    Error::source(format!(
        "the test harness failed {operation} {}: {cause}",
        path.display()
    ))
}
