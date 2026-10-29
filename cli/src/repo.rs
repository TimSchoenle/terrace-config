//! A repository's own settings: `terrace.yaml` at the root of its Git repository.
//!
//! Everything here is a setting the CLI would otherwise take from a flag, an environment variable or
//! a built-in path. The file only supplies values; which layer wins is decided by the caller, which
//! is the only place that knows whether a flag was given. [`spec/helm/v1/REPO.md`] is the normative
//! description and [`SCHEMA`] the machine-checkable one.
//!
//! The one rule that shapes this module is compatibility in both directions. A file written for a
//! newer CLI must still be readable by an older one, so a key this version does not define is
//! collected into [`RepoConfig::unknown`] rather than refused; the caller warns about it, or fails
//! under `--strict`. A file declaring a `version` this build does not support is refused outright,
//! because reading it optimistically would apply rules nobody wrote it against.
//!
//! [`spec/helm/v1/REPO.md`]: https://github.com/TimSchoenle/terrace-config/blob/main/spec/helm/v1/REPO.md

use std::path::{Path, PathBuf};

use serde_json::Value as Json;

use crate::Error;

/// The file name, at the root of the Git repository.
pub const FILE_NAME: &str = "terrace.yaml";

/// The layer version this build reads.
pub const VERSION: u64 = 1;

/// The published schema for [`FILE_NAME`], embedded so validation needs no checkout and no network.
pub const SCHEMA: &str = include_str!("../../spec/helm/v1/repo.schema.json");

/// Where an editor fetches [`SCHEMA`] from, as written into the modeline of a new file.
pub const SCHEMA_URL: &str = "https://raw.githubusercontent.com/TimSchoenle/terrace-config/main/spec/helm/v1/repo.schema.json";

/// The keys this version defines, by section. Anything else is unknown unless it starts with `x-`.
const TOP_LEVEL: &[&str] = &["version", "cli", "helm"];
const HELM: &[&str] = &["charts", "firstParty", "signer"];

/// A loaded `terrace.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoConfig {
    /// The file it was read from.
    pub path: PathBuf,
    /// The directory holding it, which every relative path in it is resolved against.
    pub root: PathBuf,
    /// The pinned CLI release, as written.
    pub cli: Option<CliPin>,
    /// The chart repository section.
    pub helm: HelmSettings,
    /// Every key this version does not define, as a dotted path, sorted within each section.
    pub unknown: Vec<String>,
}

/// `helm:` in [`RepoConfig`]. Every field is absent when the file does not set it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelmSettings {
    /// The chart tree, already resolved against [`RepoConfig::root`].
    pub charts: Option<PathBuf>,
    /// The first-party repository patterns.
    pub first_party: Option<Vec<String>>,
    /// The signing identity a fetched contract's image must carry.
    pub signer: Option<String>,
}

/// A `cli:` pin: `MAJOR.MINOR`, or `MAJOR.MINOR.PATCH`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CliPin {
    /// The major release.
    pub major: u64,
    /// The minor release.
    pub minor: u64,
    /// The patch release, when the pin names one.
    pub patch: Option<u64>,
}

impl CliPin {
    /// Reads a pin as written in the file.
    ///
    /// # Errors
    /// [`Error::Invalid`] when it is not two or three dot-separated numbers.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let parts: Vec<&str> = text.split('.').collect();
        let number = |part: &str| -> Result<u64, Error> {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(Error::Invalid(format!(
                    "`cli: {text}` is not a release: write MAJOR.MINOR or MAJOR.MINOR.PATCH"
                )));
            }
            part.parse()
                .map_err(|_| Error::Invalid(format!("`cli: {text}`: `{part}` is out of range")))
        };
        match parts.as_slice() {
            [major, minor] => Ok(Self {
                major: number(major)?,
                minor: number(minor)?,
                patch: None,
            }),
            [major, minor, patch] => Ok(Self {
                major: number(major)?,
                minor: number(minor)?,
                patch: Some(number(patch)?),
            }),
            _ => Err(Error::Invalid(format!(
                "`cli: {text}` is not a release: write MAJOR.MINOR or MAJOR.MINOR.PATCH"
            ))),
        }
    }

    /// Whether a running release satisfies this pin.
    ///
    /// A `MAJOR.MINOR` pin takes every patch of that minor. Below `1.0` a minor release may break,
    /// so the minor is compared even for a `0.x` pin and nothing looser is offered.
    pub fn accepts(self, running: Self) -> bool {
        self.major == running.major
            && self.minor == running.minor
            && self.patch.is_none_or(|patch| Some(patch) == running.patch)
    }

    /// Whether this pin names a release newer than `running`.
    pub fn is_newer_than(self, running: Self) -> bool {
        (self.major, self.minor, self.patch.unwrap_or(0))
            > (running.major, running.minor, running.patch.unwrap_or(0))
    }
}

impl std::fmt::Display for CliPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.patch {
            Some(patch) => write!(f, "{}.{}.{patch}", self.major, self.minor),
            None => write!(f, "{}.{}", self.major, self.minor),
        }
    }
}

/// The root of the Git repository holding `start`: the nearest ancestor with a `.git` entry.
///
/// A file or a directory both count, so a worktree and a submodule each resolve to their own root.
pub fn git_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|directory| directory.join(".git").exists())
        .map(Path::to_path_buf)
}

/// The `terrace.yaml` at the root of the Git repository holding `start`, if there is one.
///
/// Outside a Git repository there is no root and so no file: a file in an arbitrary parent
/// directory is never picked up by accident.
pub fn discover(start: &Path) -> Option<PathBuf> {
    let candidate = git_root(start)?.join(FILE_NAME);
    candidate.is_file().then_some(candidate)
}

/// Reads and checks a `terrace.yaml`.
///
/// # Errors
/// [`Error::Io`] when the file cannot be read. [`Error::Invalid`] when it is not YAML, declares no
/// `version` or one this build does not support, or fails [`SCHEMA`]. The version is checked before
/// the schema, so a file written for a later layer is refused for that reason rather than for
/// whatever its new shape happens to break.
pub fn load(path: &Path) -> Result<RepoConfig, Error> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path.display(), e))?;
    parse(&text, path)
}

/// [`load`], over text already read.
///
/// # Errors
/// As [`load`], except for reading the file.
pub fn parse(text: &str, path: &Path) -> Result<RepoConfig, Error> {
    let document = crate::helm::declaration::read_yaml(text, path)?;
    let shown = path.display();

    let Some(fields) = document.as_object() else {
        return Err(Error::Invalid(format!(
            "{shown}: is not a mapping. Start the file with `version: {VERSION}`."
        )));
    };
    match fields.get("version") {
        None => {
            return Err(Error::Invalid(format!(
                "{shown}: declares no `version`. Add `version: {VERSION}` as its first key."
            )));
        }
        Some(Json::Number(number)) if number.as_u64() == Some(VERSION) => {}
        Some(other) => {
            return Err(Error::Invalid(format!(
                "{shown}: `version: {other}` is not a layer version this terrace-contract reads \
                 (it reads {VERSION}). Install the release that introduced it, or rewrite the file \
                 for version {VERSION}."
            )));
        }
    }

    let errors = schema_errors(&document)?;
    if !errors.is_empty() {
        return Err(Error::Invalid(format!(
            "{shown}: does not satisfy spec/helm/v1/repo.schema.json:\n  {}",
            errors.join("\n  ")
        )));
    }

    let root = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let mut unknown = Vec::new();
    collect_unknown(fields, TOP_LEVEL, "", &mut unknown);

    let cli = fields
        .get("cli")
        .and_then(Json::as_str)
        .map(CliPin::parse)
        .transpose()
        .map_err(|error| Error::Invalid(format!("{shown}: {error}")))?;

    let mut helm = HelmSettings::default();
    if let Some(section) = fields.get("helm").and_then(Json::as_object) {
        collect_unknown(section, HELM, "helm.", &mut unknown);
        helm.charts = section
            .get("charts")
            .and_then(Json::as_str)
            .map(|charts| root.join(charts));
        helm.first_party = section
            .get("firstParty")
            .and_then(Json::as_array)
            .map(|patterns| {
                patterns
                    .iter()
                    .filter_map(Json::as_str)
                    .map(str::to_owned)
                    .collect()
            });
        helm.signer = section
            .get("signer")
            .and_then(Json::as_str)
            .map(str::to_owned);
    }

    Ok(RepoConfig {
        path: path.to_path_buf(),
        root,
        cli,
        helm,
        unknown,
    })
}

/// Every way a document fails [`SCHEMA`].
///
/// # Errors
/// [`Error::Invalid`] when the embedded schema does not compile, which is a defect in this build.
pub fn schema_errors(document: &Json) -> Result<Vec<String>, Error> {
    let schema: Json = serde_json::from_str(SCHEMA)
        .map_err(|e| Error::Invalid(format!("the embedded repo schema is not JSON: {e}")))?;
    let validator = jsonschema::validator_for(&schema).map_err(|e| {
        Error::Invalid(format!(
            "the embedded repo schema does not compile, which is a defect in this build: {e}"
        ))
    })?;
    Ok(validator
        .iter_errors(document)
        .map(|error| {
            let at = error.instance_path().to_string();
            let at = if at.is_empty() {
                "<file>".to_owned()
            } else {
                at
            };
            format!("{at}: {error}")
        })
        .collect())
}

fn collect_unknown(
    fields: &serde_json::Map<String, Json>,
    known: &[&str],
    prefix: &str,
    unknown: &mut Vec<String>,
) {
    for name in fields.keys() {
        if !known.contains(&name.as_str()) && !name.starts_with("x-") {
            unknown.push(format!("{prefix}{name}"));
        }
    }
}

/// Where a resolved setting came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A command-line flag.
    Flag,
    /// An environment variable.
    Environment,
    /// `terrace.yaml`.
    File,
    /// The built-in default.
    Default,
}

impl Source {
    /// The name `config show` prints.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Environment => "environment",
            Self::File => "terrace.yaml",
            Self::Default => "default",
        }
    }
}

/// The settings a command runs with: a loaded file, or none.
///
/// Every accessor takes what the command line supplied and falls back through the file to the
/// built-in default. A built-in path is resolved against the directory holding the file when there
/// is one, so a repository that adopts the file gets the same answer from any working directory;
/// without a file it stays relative to the working directory, as it always was.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// The file, when one was found or named.
    pub file: Option<RepoConfig>,
}

impl Settings {
    /// No file: every setting is a flag, a variable or a default.
    pub fn none() -> Self {
        Self::default()
    }

    /// What a built-in relative path is resolved against.
    fn base(&self) -> &Path {
        self.file.as_ref().map_or(Path::new(""), |file| &file.root)
    }

    /// The chart tree.
    pub fn charts(&self, given: Option<PathBuf>) -> (PathBuf, Source) {
        if let Some(path) = given {
            return (path, Source::Flag);
        }
        if let Some(path) = self.file.as_ref().and_then(|file| file.helm.charts.clone()) {
            return (path, Source::File);
        }
        (self.base().join(crate::helm::CHARTS_DIR), Source::Default)
    }

    /// The first-party repository patterns: a named list file, the file's own list, or the
    /// default list file.
    ///
    /// # Errors
    /// As [`crate::helm::coverage::first_party_patterns`], when a list file is the source and
    /// cannot be read.
    pub fn first_party(&self, given: Option<&Path>) -> Result<(Vec<String>, Source), Error> {
        if let Some(path) = given {
            return Ok((
                crate::helm::coverage::first_party_patterns(path)?,
                Source::Flag,
            ));
        }
        if let Some(patterns) = self
            .file
            .as_ref()
            .and_then(|file| file.helm.first_party.clone())
        {
            return Ok((patterns, Source::File));
        }
        let path = self.default_first_party();
        Ok((
            crate::helm::coverage::first_party_patterns(&path)?,
            Source::Default,
        ))
    }

    /// The default first-party list file, resolved like every built-in path.
    pub fn default_first_party(&self) -> PathBuf {
        self.base().join(crate::helm::FIRST_PARTY)
    }

    /// The signing identity, when anything sets one.
    pub fn signer(&self, given: Option<String>) -> Option<(String, Source)> {
        given.map(|signer| (signer, Source::Flag)).or_else(|| {
            self.file
                .as_ref()
                .and_then(|file| file.helm.signer.clone())
                .map(|signer| (signer, Source::File))
        })
    }
}

/// The running release as a pin, for comparing against `cli:`.
///
/// # Errors
/// [`Error::Invalid`] when the build's own version is not `MAJOR.MINOR.PATCH`, which is a defect
/// in the build rather than in anything a user wrote.
pub fn running() -> Result<CliPin, Error> {
    let version = env!("CARGO_PKG_VERSION");
    let release = version.split(['-', '+']).next().unwrap_or(version);
    CliPin::parse(release)
}

/// The file `config init` writes.
///
/// `cli` pins the running release's minor. `first_party` carries the patterns of an existing
/// first-party list, so adopting the file moves that list into it rather than leaving two sources.
pub fn template(running: CliPin, first_party: &[String]) -> String {
    let mut text = format!(
        "# yaml-language-server: $schema={SCHEMA_URL}\n\
         # Settings for terrace-contract. Every key but `version` is optional; see\n\
         # spec/helm/v1/REPO.md in TimSchoenle/terrace-config for each key and its default.\n\
         version: {VERSION}\n\
         cli: \"{}.{}\"\n\
         \n\
         helm:\n  charts: charts\n",
        running.major, running.minor
    );
    if !first_party.is_empty() {
        text.push_str("  firstParty:\n");
        for pattern in first_party {
            text.push_str("    - ");
            text.push_str(&quote_if_needed(pattern));
            text.push('\n');
        }
    }
    text.push_str(
        "  # signer: the certificate identity, as a regular expression, that every image's\n  \
         # signature must carry before `pull` fetches its contract.\n",
    );
    text
}

/// A YAML scalar for a pattern, quoted when a plain scalar would read as something else.
fn quote_if_needed(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '-' | '_' | '*'))
        && !text.starts_with('*');
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "''"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{CliPin, FILE_NAME, VERSION, discover, parse, template};

    fn read(text: &str) -> Result<super::RepoConfig, crate::Error> {
        parse(text, Path::new("repo/terrace.yaml"))
    }

    #[test]
    fn a_minimal_file_reads_with_nothing_set() {
        let config = read("version: 1\n").expect("minimal");
        assert_eq!(config.root, PathBuf::from("repo"));
        assert_eq!(config.cli, None);
        assert_eq!(config.helm, super::HelmSettings::default());
        assert_eq!(config.unknown, Vec::<String>::new());
    }

    #[test]
    fn every_key_is_read_and_paths_resolve_against_the_file() {
        let config = read(
            "version: 1\ncli: \"0.8\"\nhelm:\n  charts: deploy/charts\n  firstParty: [ghcr.io/a/*, b]\n  signer: 'https://x@refs/heads/main'\n",
        )
        .expect("full");
        assert_eq!(
            config.cli,
            Some(CliPin {
                major: 0,
                minor: 8,
                patch: None
            })
        );
        assert_eq!(
            config.helm.charts,
            Some(PathBuf::from("repo/deploy/charts"))
        );
        assert_eq!(
            config.helm.first_party,
            Some(vec!["ghcr.io/a/*".to_owned(), "b".to_owned()])
        );
        assert_eq!(
            config.helm.signer.as_deref(),
            Some("https://x@refs/heads/main")
        );
    }

    #[test]
    fn a_missing_version_is_refused() {
        let error = read("helm: {}\n").expect_err("no version");
        assert!(
            error.to_string().contains("declares no `version`"),
            "{error}"
        );
    }

    #[test]
    fn a_later_version_is_refused_before_its_shape_is_judged() {
        let error = read("version: 2\nhelm: 7\n").expect_err("version 2");
        assert!(error.to_string().contains("`version: 2`"), "{error}");
        assert!(!error.to_string().contains("schema"), "{error}");
    }

    #[test]
    fn a_wrong_type_is_refused_by_the_schema() {
        let error = read("version: 1\nhelm:\n  charts: 3\n").expect_err("charts must be a path");
        assert!(error.to_string().contains("/helm/charts"), "{error}");
    }

    #[test]
    fn unknown_keys_are_collected_and_extensions_are_not() {
        let config = read("version: 1\nlater: true\nx-team: a\nhelm:\n  render: {}\n  x-note: b\n")
            .expect("unknown keys are read, not refused");
        assert_eq!(
            config.unknown,
            vec!["later".to_owned(), "helm.render".to_owned()]
        );
    }

    #[test]
    fn a_pin_accepts_its_own_minor_and_nothing_else() {
        let pin = CliPin::parse("0.8").expect("minor pin");
        let running = |text| CliPin::parse(text).expect("release");
        assert!(pin.accepts(running("0.8.0")));
        assert!(pin.accepts(running("0.8.9")));
        assert!(!pin.accepts(running("0.9.0")));
        assert!(!pin.accepts(running("1.8.0")));
        let exact = CliPin::parse("0.8.2").expect("exact pin");
        assert!(exact.accepts(running("0.8.2")));
        assert!(!exact.accepts(running("0.8.3")));
        assert!(pin.is_newer_than(running("0.7.9")));
        assert!(!pin.is_newer_than(running("0.8.0")));
    }

    #[test]
    fn a_malformed_pin_is_refused() {
        for text in ["0", "0.8.1.2", "v0.8", "0.x", "0..1", ""] {
            assert!(CliPin::parse(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn the_template_reads_back_as_written() {
        let text = template(
            CliPin {
                major: 0,
                minor: 8,
                patch: Some(1),
            },
            &["ghcr.io/a/*".to_owned(), "*odd".to_owned()],
        );
        assert!(
            text.starts_with("# yaml-language-server: $schema="),
            "{text}"
        );
        let config = read(&text).expect("the template is a valid file");
        assert_eq!(
            config.cli,
            Some(CliPin {
                major: 0,
                minor: 8,
                patch: None
            })
        );
        assert_eq!(
            config.helm.first_party,
            Some(vec!["ghcr.io/a/*".to_owned(), "*odd".to_owned()])
        );
        assert_eq!(config.unknown, Vec::<String>::new());
        assert_eq!(VERSION, 1);
    }

    #[test]
    fn discovery_stops_at_the_git_root() {
        let scratch = std::env::temp_dir().join(format!("terrace-repo-{}", std::process::id()));
        let nested = scratch.join("repo/charts/a");
        std::fs::create_dir_all(&nested).expect("scratch tree");
        std::fs::create_dir_all(scratch.join("repo/.git")).expect("git dir");
        std::fs::write(scratch.join(FILE_NAME), "version: 1\n").expect("outside file");

        assert_eq!(
            discover(&nested),
            None,
            "a file above the Git root is never read"
        );
        std::fs::write(scratch.join("repo").join(FILE_NAME), "version: 1\n").expect("root file");
        assert_eq!(
            discover(&nested),
            Some(scratch.join("repo").join(FILE_NAME))
        );
        std::fs::remove_dir_all(&scratch).expect("cleanup");
    }
}
