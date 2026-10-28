//! How a run finds terrace.yaml, and the settings that resolve from it, the environment and flags.

use std::path::PathBuf;

use clap::Args;
use terrace_contract::Error;
use terrace_contract::repo::{self, Settings, Source};

/// The signing identity `pull` verifies against, from whichever layer sets one.
pub(crate) fn required_signer(settings: &Settings, given: Option<String>) -> Result<String, Error> {
    const MISSING: &str = "no signing identity: pass --signer, set CONTRACT_SIGNER, or set \
                           helm.signer in terrace.yaml. There is no default, because one would \
                           accept a signature nobody chose to trust.";
    settings
        .signer(given)
        .map(|(signer, _)| signer)
        .ok_or_else(|| Error::Invalid(MISSING.to_owned()))
}

/// How this run finds terrace.yaml, before it has been read.
///
/// Only the commands that take a setting from the file read it, so a document-only command
/// (`render`, `conform`, ...) never fails over a repository file it does not use.
#[derive(Args)]
pub(crate) struct Repo {
    /// The terrace.yaml to read, instead of the one at the root of the Git repository.
    #[arg(
        id = "config",
        long = "config",
        global = true,
        value_name = "FILE",
        env = "TERRACE_CONFIG"
    )]
    pub(crate) named: Option<PathBuf>,

    /// Fail, rather than warn, on a key in terrace.yaml this release does not read.
    ///
    /// `TERRACE_STRICT` takes 1/0, true/false, yes/no or on/off.
    #[arg(
        long,
        global = true,
        env = "TERRACE_STRICT",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    strict: bool,
}

impl Repo {
    /// The file this run reads: the one named, else the one at the Git root, else none.
    pub(crate) fn path(&self) -> Result<Option<PathBuf>, Error> {
        if let Some(named) = &self.named {
            // Named and missing is an error, where absent from the repository is not: a name is a
            // statement that the file exists, and silently running on defaults would hide a typo.
            if !named.is_file() {
                return Err(Error::Invalid(format!(
                    "{}: no such file, named by --config or TERRACE_CONFIG",
                    named.display()
                )));
            }
            return Ok(Some(named.clone()));
        }
        let here = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        Ok(repo::discover(&here))
    }

    /// The file, read without judging its unknown keys.
    pub(crate) fn load(&self) -> Result<Settings, Error> {
        Ok(Settings {
            file: self.path()?.as_deref().map(repo::load).transpose()?,
        })
    }

    /// The file, read, with every unknown key warned about or, under `--strict`, refused.
    pub(crate) fn settings(&self) -> Result<Settings, Error> {
        let settings = self.load()?;
        if let Some(file) = &settings.file
            && !file.unknown.is_empty()
        {
            let lines = unknown_key_lines(file)?;
            if self.strict {
                return Err(Error::Invalid(format!(
                    "{} (refused under --strict):\n  {}",
                    file.path.display(),
                    lines.join("\n  ")
                )));
            }
            for line in &lines {
                eprintln!("warning: {}: ignored: {line}", file.path.display());
            }
        }
        Ok(settings)
    }

    pub(crate) fn charts(&self, given: Option<PathBuf>) -> Result<PathBuf, Error> {
        Ok(self.settings()?.charts(given).0)
    }
}

/// One line per key this release does not read, saying why it might be there.
pub(crate) fn unknown_key_lines(file: &repo::RepoConfig) -> Result<Vec<String>, Error> {
    let running = repo::running()?;
    // An older CLI cannot know which release introduced a key, but the file's own pin can say the
    // repository expects a newer one, which is the likeliest reason and the actionable one.
    let hint = file
        .cli
        .filter(|pin| pin.is_newer_than(running))
        .map(|pin| format!("; the file pins cli {pin} and this is {running}, so upgrade"))
        .unwrap_or_default();
    Ok(file
        .unknown
        .iter()
        .map(|key| format!("`{key}` is not a key this terrace-contract reads{hint}"))
        .collect())
}

/// A variable's value, treating an empty one as unset the way clap does.
pub(crate) fn variable(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// The chart tree for a command with no `--charts` flag of its own, attributed to its layer.
pub(crate) fn environment_charts(settings: &Settings) -> (PathBuf, Source) {
    variable("TERRACE_CHARTS").map_or_else(
        || settings.charts(None),
        |value| (PathBuf::from(value), Source::Environment),
    )
}

/// The signing identity for a command with no `--signer` flag of its own, attributed to its layer.
pub(crate) fn environment_signer(settings: &Settings) -> Option<(String, Source)> {
    variable("CONTRACT_SIGNER")
        .map(|value| (value, Source::Environment))
        .or_else(|| settings.signer(None))
}
