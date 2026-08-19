//! What was asked for: a format, how much of the configuration, and what build this is.

use std::fmt;

use super::Format;
use crate::schema::App;

/// The one line to print when an argument is refused.
pub const USAGE: &str = "usage: config-schema \
                         [--format json|markdown|markdown-loader|markdown-keys|toml|\
                         json-schema|contract|labels|dockerfile] \
                         [--only <key-prefix>] [--path <in-image-path>] \
                         [--version <release>] [--revision <commit>] [--created <rfc3339>]";

/// A generator invocation, parsed.
///
/// Deliberately a plain value with public constructors rather than something only
/// [`Self::from_env`] can produce: a service already parsing arguments with `clap` builds one with
/// [`Self::new`] and the setters, and gets [`Cli::render`](super::Cli::render) without inheriting
/// this module's argument syntax. The two entry points meet at this type.
///
/// # Reproducibility
///
/// Nothing here is read from the environment, and that is the property the whole design protects:
/// a documentation job on a laptop and a container build on a runner produce the same bytes.
/// [`version`](Self::version), [`revision`](Self::revision) and [`created`](Self::created) are the
/// three things that legitimately differ between builds of one source tree, so they are arguments
/// — passed, they make the difference explicit; omitted, `--format contract` is byte-reproducible
/// and can be committed and diffed in review.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Request {
    format: Format,
    only: String,
    path: Option<String>,
    version: Option<String>,
    revision: Option<String>,
    created: Option<String>,
}

impl Request {
    /// A request for one format, over the whole configuration, of no particular build.
    #[must_use]
    pub fn new(format: Format) -> Self {
        Self {
            format,
            ..Self::default()
        }
    }

    /// Parse an argument list — the one [`USAGE`] describes.
    ///
    /// Takes the arguments themselves, without the program name, so it can be tested and so a
    /// caller can feed it something other than `std::env::args`. [`Self::from_env`] is the usual
    /// entry point.
    ///
    /// # Errors
    /// Returns [`UsageError`] naming the argument and what was expected of it. Every message ends
    /// with [`USAGE`], because the reader is looking at a build log rather than a terminal they
    /// can ask again.
    pub fn parse<I>(args: I) -> Result<Self, UsageError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut request = Self::default();
        let mut args = args.into_iter();

        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--format" => {
                    let spelling = value(&mut args, "--format", "a rendering")?;
                    request.format = spelling
                        .parse()
                        .map_err(|error| UsageError(format!("{error}")))?;
                }
                "--only" => request.only = value(&mut args, "--only", "a key prefix")?,
                "--path" => request.path = Some(value(&mut args, "--path", "a path")?),
                "--version" => request.version = Some(value(&mut args, "--version", "a release")?),
                "--revision" => {
                    request.revision = Some(value(&mut args, "--revision", "a commit")?);
                }
                "--created" => {
                    request.created = Some(value(&mut args, "--created", "a timestamp")?);
                }
                other => return Err(UsageError(format!("unknown argument `{other}`"))),
            }
        }

        request.validate()?;
        Ok(request)
    }

    /// [`Self::parse`] over this process's arguments.
    ///
    /// # Errors
    /// As [`Self::parse`].
    pub fn from_env() -> Result<Self, UsageError> {
        Self::parse(std::env::args().skip(1))
    }

    /// The rendering to emit.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// The subtree to keep, or empty for the whole configuration.
    #[must_use]
    pub fn only(&self) -> &str {
        &self.only
    }

    /// Where the contract is embedded in the image, for the label renderings.
    ///
    /// [`DEFAULT_PATH`](crate::schema::DEFAULT_PATH) unless `--path` said otherwise. An argument
    /// rather than a constant only because a `scratch` image with an unusual layout exists; almost
    /// nothing should pass it, and a build that does must pass the same value to its `COPY`.
    #[must_use]
    pub fn path(&self) -> &str {
        self.path.as_deref().unwrap_or(crate::schema::DEFAULT_PATH)
    }

    /// The release this build is of, if it was told.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// The commit this build is of, if it was told.
    #[must_use]
    pub fn revision(&self) -> Option<&str> {
        self.revision.as_deref()
    }

    /// When this build happened, if it was told.
    #[must_use]
    pub fn created(&self) -> Option<&str> {
        self.created.as_deref()
    }

    /// Set the rendering.
    #[must_use]
    pub fn with_format(mut self, format: Format) -> Self {
        self.format = format;
        self
    }

    /// Keep only the keys under this prefix. Empty for the whole configuration.
    #[must_use]
    pub fn with_only(mut self, only: impl Into<String>) -> Self {
        self.only = only.into();
        self
    }

    /// Where the contract is embedded in the image.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// The release this build is of, as the image tag spells it.
    ///
    /// This and the two below complete what [`Self::new`] starts, so a consumer parsing arguments
    /// with `clap` — or one that has its own `--scope` or `--service` flag and therefore cannot
    /// hand its whole argument list to [`Self::parse`] — can build every field [`stamp`] reads.
    /// Without them the second layer was usable for a documentation job and not for a container
    /// build, which is the half that needed it.
    ///
    /// [`stamp`]: Self::stamp
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// The commit this build is of.
    #[must_use]
    pub fn with_revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = Some(revision.into());
        self
    }

    /// When this build happened, RFC 3339.
    #[must_use]
    pub fn with_created(mut self, created: impl Into<String>) -> Self {
        self.created = Some(created.into());
        self
    }

    /// The release, the commit and the moment this build is of.
    ///
    /// Each is applied only if it was given, so an [`App`] carrying a compile-time default —
    /// `concat!("v", env!("CARGO_PKG_VERSION"))` is the usual one — keeps it when the argument is
    /// absent, and a build passing `--version "$TAG"` overrides it.
    #[must_use]
    pub fn stamp(&self, mut app: App) -> App {
        if let Some(version) = &self.version {
            app = app.version(version);
        }
        if let Some(revision) = &self.revision {
            app = app.revision(revision);
        }
        if let Some(created) = &self.created {
            app = app.created(created);
        }
        app
    }

    /// Refuse the combinations that would produce a plausible-looking wrong answer.
    ///
    /// # Errors
    /// Returns [`UsageError`] if `--only` was given for a rendering it cannot slice — either a
    /// whole-image format, where the slice would be published as a false claim (see
    /// [`Format::whole_image`]), or one that carries no keys at all, where it would be silently
    /// ignored (see [`Format::reads_keys`]).
    pub fn validate(&self) -> Result<(), UsageError> {
        if self.only.is_empty() {
            return Ok(());
        }

        if self.format.whole_image() {
            return Err(UsageError(format!(
                "`--only` slices a configuration and `--format {}` describes a whole image; a \
                 contract built from a slice would claim the image does not read the keys it cut",
                self.format
            )));
        }

        if !self.format.reads_keys() {
            return Err(UsageError(format!(
                "`--format {}` renders the variables that select the loader's layers, not the \
                 keys read out of them, so `--only` would have sliced nothing and changed no \
                 output",
                self.format
            )));
        }

        Ok(())
    }
}

/// The value after `flag`, or a message naming what was expected.
///
/// An empty value is refused rather than recorded. A build passing `--version "$VERSION"` with
/// nothing in `VERSION` means the argument failed to interpolate, and a contract claiming the
/// empty release is worse than a build that stops.
fn value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
    expected: &str,
) -> Result<String, UsageError> {
    args.next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| UsageError(format!("`{flag}` takes {expected}")))
}

/// An argument list this generator cannot act on.
///
/// [`Display`](fmt::Display) appends [`USAGE`], so printing one is the whole error path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError(String);

impl UsageError {
    /// The complaint on its own, without [`USAGE`] after it.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}; {USAGE}", self.0)
    }
}

impl std::error::Error for UsageError {}
