//! A mounted directory, in the shapes Kubernetes actually produces one.

use std::path::PathBuf;

use super::jail::Jail;
use crate::error::Error;

/// The timestamped directory a projected volume keeps its current generation in.
///
/// A fixed name rather than a real timestamp: a test that pins a layout should produce the same
/// bytes on every run, and nothing in the loader parses it. The whole rule is that a dot-prefixed
/// entry is not a key.
const DEFAULT_GENERATION: &str = "..2026_08_02_10_00_00";

/// What a projected volume holds in `..data` when `..data` is not a symlink.
///
/// Deliberately not valid as anything: if it ever reaches the configuration, the test reading it
/// fails.
const DECOY: &str = "not a key";

/// How the entries of a volume are laid out on disk.
///
/// The three shapes are not stylistic variants. Each pins a different rule, and the distinction
/// between the last two is one of the reasons this crate exists, as
/// [`SecretsDir::read`](crate::provider::SecretsDir::read) explains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// Ordinary files in an ordinary directory: `docker run -v`, a hand-rolled mount, or a
    /// developer's `mkdir secrets`. The default.
    #[default]
    Plain,
    /// The *names* a projected volume has, with the keys written as regular files.
    ///
    /// That is a `..data` entry and a timestamped generation directory beside the real keys.
    ///
    /// Portable, and what pins the skipping rules: a dot-prefixed name is not a key, and a
    /// subdirectory is not a key. It does **not** reproduce the mount, which is why the variant
    /// below exists.
    Projected,
    /// A projected volume as the kubelet actually writes one: the generation directory holds the
    /// contents, `..data` is a symlink to it, and every key is a symlink to `..data/<key>`.
    ///
    /// Only a real symlink reproduces the failure this layout pins. `DirEntry::metadata()` does not
    /// follow symlinks, despite its name, so a provider using it reports every real key as "not a
    /// file" and leaves the layer silently empty while [`Layout::Projected`] still passes. Every
    /// service then boots on compiled defaults.
    ///
    /// Unix only. [`Sandbox::symlink`](super::Sandbox::symlink) says why that is the honest
    /// signature rather than a gap. A test using it carries `#[cfg(unix)]`.
    #[cfg(unix)]
    Symlinked,
}

/// A directory of files, and the environment variable that points at it.
///
/// Built by [`Jail::volume`], [`Jail::secrets_volume`] or [`Jail::config_volume`], and written by
/// [`Volume::create`]. Nothing touches the filesystem until then, so the whole shape of a mount
/// reads as one expression:
///
/// ```no_run
/// # fn main() -> Result<(), terrace_config::Error> {
/// # let harness = terrace_config::testing::Harness::new("TEST_");
/// # harness.try_run(|jail| {
/// jail.secrets_volume()
///     .file("auth__jwt_secret", "from-the-volume")
///     .stray_dir("nested")
///     .projected()
///     .create()?;
/// # Ok(()) })
/// # }
/// ```
///
/// The two lifetimes are the jail this will set a variable on and the `figment::Jail` that jail
/// borrows. Neither is ever named by a caller.
#[derive(Debug)]
pub struct Volume<'jail, 'figment> {
    /// Where the variable gets set, and where the sandbox comes from.
    jail: &'jail mut Jail<'figment>,
    /// The directory, relative to the sandbox root.
    dir: PathBuf,
    /// The variable to point at the directory, if any.
    wire: Option<String>,
    /// The files that are meant to be read as configuration.
    entries: Vec<(String, Vec<u8>)>,
    /// Files that must **not** be read as configuration.
    stray_files: Vec<(String, Vec<u8>)>,
    /// Directories that must **not** be read as configuration.
    stray_dirs: Vec<String>,
    /// The name of the generation directory, for the projected layouts.
    generation: String,
    /// How the entries are laid out.
    layout: Layout,
}

impl<'jail, 'figment> Volume<'jail, 'figment> {
    /// A volume at `dir`, relative to the sandbox root, wired to `wire` if one is given.
    pub(super) fn new(
        jail: &'jail mut Jail<'figment>,
        dir: impl Into<PathBuf>,
        wire: Option<String>,
    ) -> Self {
        Self {
            jail,
            dir: dir.into(),
            wire,
            entries: Vec::new(),
            stray_files: Vec::new(),
            stray_dirs: Vec::new(),
            generation: DEFAULT_GENERATION.to_owned(),
            layout: Layout::default(),
        }
    }

    /// Adds a file the loader is meant to read.
    ///
    /// In a secrets volume the name is a key in its file spelling, such as `auth__jwt_secret`, with
    /// the loader's own nesting separator. In a configuration volume it is a fragment name such as
    /// `10-base.toml`, merged in name order.
    #[must_use]
    pub fn file(mut self, name: impl Into<String>, contents: impl Into<Vec<u8>>) -> Self {
        self.entries.push((name.into(), contents.into()));
        self
    }

    /// Adds a file the loader must **not** read, beside the real ones.
    ///
    /// A `ConfigMap` volume carries `..data` and often a `README.md` in the same directory as the
    /// fragments. The assertion worth making is that neither reaches the configuration.
    ///
    /// In a *secrets* volume the only stray a file name can express is a dot-prefixed one. Every
    /// other file there is a key, because that is what a secrets directory is. A `README.md` beside
    /// the keys is a key called `readme.md`, and the loader refuses it by name rather than skipping
    /// it.
    #[must_use]
    pub fn stray_file(mut self, name: impl Into<String>, contents: impl Into<Vec<u8>>) -> Self {
        self.stray_files.push((name.into(), contents.into()));
        self
    }

    /// Adds a subdirectory the loader must **not** descend into or read as a key.
    #[must_use]
    pub fn stray_dir(mut self, name: impl Into<String>) -> Self {
        self.stray_dirs.push(name.into());
        self
    }

    /// Renames the generation directory of a projected layout.
    ///
    /// Defaults to a fixed timestamp.
    ///
    /// Worth setting when a test builds two generations and swaps `..data` between them, which is
    /// what a rotated `Secret` looks like on disk.
    #[must_use]
    pub fn generation(mut self, name: impl Into<String>) -> Self {
        self.generation = name.into();
        self
    }

    /// Lays the entries out as ordinary files in an ordinary directory.
    ///
    /// The default, [`Layout::Plain`].
    #[must_use]
    pub fn plain(self) -> Self {
        self.layout(Layout::Plain)
    }

    /// Lays the entries out with a projected volume's *names* around them, as
    /// [`Layout::Projected`].
    #[must_use]
    pub fn projected(self) -> Self {
        self.layout(Layout::Projected)
    }

    /// Lays the entries out as the kubelet does, with real symlinks, as [`Layout::Symlinked`].
    ///
    /// Unix only. A test calling it carries `#[cfg(unix)]`.
    #[cfg(unix)]
    #[must_use]
    pub fn symlinked(self) -> Self {
        self.layout(Layout::Symlinked)
    }

    /// Lays the entries out as `layout` says.
    #[must_use]
    pub fn layout(mut self, layout: Layout) -> Self {
        self.layout = layout;
        self
    }

    /// Points `variable` at the directory once it is created, replacing whatever this volume was
    /// going to set.
    ///
    /// A volume is wired by name rather than by role because a test for
    /// [`Terrace::secrets_dir_var`](crate::Terrace::secrets_dir_var) has to mount a real volume
    /// under the renamed variable *and* a decoy under the derived one, and only one of the two can
    /// be "the secrets volume".
    #[must_use]
    pub fn wire_to(mut self, variable: impl Into<String>) -> Self {
        self.wire = Some(variable.into());
        self
    }

    /// Creates the directory without pointing any variable at it.
    #[must_use]
    pub fn unwired(mut self) -> Self {
        self.wire = None;
        self
    }

    /// Writes the whole volume, sets the wired variable if there is one, and returns the
    /// directory's absolute path.
    ///
    /// # Errors
    /// [`Error::Source`] if any part of the layout cannot be created.
    pub fn create(self) -> Result<PathBuf, Error> {
        let sandbox = self.jail.sandbox();
        let root = sandbox.create_dir(&self.dir)?;

        match self.layout {
            Layout::Plain => {
                for (name, contents) in &self.entries {
                    sandbox.write(self.dir.join(name), contents)?;
                }
            }
            Layout::Projected => {
                // The generation directory holds the contents, as it does in a cluster, and the
                // keys are regular files beside it rather than symlinks into it. `..data` is a
                // plain file for the same reason: what this layout pins is that a dot-prefixed
                // *name* is not a key, whatever kind of entry it is.
                sandbox.create_dir(self.dir.join(&self.generation))?;
                for (name, contents) in &self.entries {
                    sandbox.write(self.dir.join(&self.generation).join(name), contents)?;
                    sandbox.write(self.dir.join(name), contents)?;
                }
                sandbox.write(self.dir.join("..data"), DECOY)?;
            }
            #[cfg(unix)]
            Layout::Symlinked => {
                sandbox.create_dir(self.dir.join(&self.generation))?;
                for (name, contents) in &self.entries {
                    sandbox.write(self.dir.join(&self.generation).join(name), contents)?;
                }
                // Relative, and through `..data` rather than straight at the generation
                // directory: swapping that one symlink is what swaps every key at once, and the
                // atomicity of that swap is the whole design of a projected volume.
                sandbox.symlink(self.dir.join("..data"), &self.generation)?;
                for (name, _) in &self.entries {
                    sandbox.symlink(self.dir.join(name), PathBuf::from("..data").join(name))?;
                }
            }
        }

        for (name, contents) in &self.stray_files {
            sandbox.write(self.dir.join(name), contents)?;
        }
        for name in &self.stray_dirs {
            sandbox.create_dir(self.dir.join(name))?;
        }

        if let Some(variable) = &self.wire {
            self.jail.env(variable, root.display());
        }
        Ok(root)
    }
}
