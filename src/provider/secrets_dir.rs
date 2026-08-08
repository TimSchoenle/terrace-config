//! A directory of key-named files, which is what a Kubernetes `Secret` volume looks like.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use figment::value::{Dict, Map, Value};
use figment::{Metadata, Profile, Provider};

use crate::dialect::Dialect;
use crate::error::Error;
use crate::provider::{FileValue, insert_nested, read_value};

/// Every key-named file directly inside one directory, read at construction.
///
/// Reading happens in [`SecretsDir::read`] rather than in [`Provider::data`] so that a mount
/// that cannot be read is a typed [`Error`] naming the path, rather than a `figment::Error`
/// raised somewhere inside an extraction.
#[derive(Debug, Clone)]
pub struct SecretsDir {
    /// The directory that was read. Retained so a reload knows what to watch.
    dir: PathBuf,
    /// The values found, keyed by figment key path.
    values: BTreeMap<String, FileValue>,
}

impl SecretsDir {
    /// Read every key-named file directly inside `dir`.
    ///
    /// Entries whose name begins with `.` are skipped, and so is anything that is not a regular
    /// file. Both are what makes a Kubernetes projected `Secret` volume work: it holds a
    /// `..data` symlink pointing at a timestamped directory, plus one symlink per key. The
    /// per-key symlinks must be followed, which is why this uses `fs::metadata` and **not**
    /// `DirEntry::metadata()`: despite the name the latter does *not* traverse symlinks — it
    /// carries `symlink_metadata` semantics — so it classifies every real key as "not a file"
    /// and silently yields an empty layer. That is not hypothetical: it is what shipped, and
    /// every service then booted on compiled defaults and died naming the first required field
    /// it was missing.
    ///
    /// `origin` names whatever pointed at `dir` — an environment variable, a CLI flag — and is
    /// quoted back in any error, because an operator who mounted the wrong path needs to be
    /// told which knob they set wrong.
    ///
    /// # Errors
    /// Returns [`Error::Source`] if the directory cannot be read, if a file's contents are not
    /// UTF-8, or if a file name is not a usable key — it contains the nesting separator's
    /// disallowed `.`, or it names a key reserved by `dialect`.
    pub fn read(origin: &str, dir: impl Into<PathBuf>, dialect: &Dialect) -> Result<Self, Error> {
        let dir = dir.into();
        let entries = std::fs::read_dir(&dir).map_err(|e| {
            Error::source(format!(
                "{origin} is {}, which could not be read: {e}",
                dir.display()
            ))
        })?;

        let mut values = BTreeMap::new();
        for entry in entries {
            let entry =
                entry.map_err(|e| Error::source(format!("reading {}: {e}", dir.display())))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            // Follows symlinks; see the note above. A dangling link yields `Err` and is
            // skipped, which is the same outcome as for a genuinely absent target.
            if !std::fs::metadata(&path).is_ok_and(|m| m.is_file()) {
                continue;
            }

            let key = key_from_name(&name, &path, dialect)?;
            values.insert(
                key,
                FileValue {
                    value: read_value(&path)?,
                    path,
                },
            );
        }

        Ok(Self { dir, values })
    }

    /// The directory this layer was read from.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The values this layer supplies, keyed by figment key path (`auth.jwt_secret`).
    #[must_use]
    pub fn values(&self) -> &BTreeMap<String, FileValue> {
        &self.values
    }

    /// Whether the directory held no usable keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl Provider for SecretsDir {
    fn metadata(&self) -> Metadata {
        Metadata::named(format!("secrets directory {}", self.dir.display()))
    }

    fn data(&self) -> Result<Map<Profile, Dict>, figment::Error> {
        let mut dict = Dict::new();
        for (key, value) in &self.values {
            insert_nested(&mut dict, key, Value::from(value.value.as_str()));
        }
        Ok(Profile::Default.collect(dict))
    }
}

/// The figment key a secrets-directory file name denotes.
///
/// # Errors
/// A name containing `.` — Kubernetes allows it in a `Secret` key, but the nesting separator
/// here is the dialect's, so `auth.jwt_secret` would silently mean something other than it
/// looks like. Or a name that spells a reserved key, which a file may not supply.
fn key_from_name(name: &str, path: &Path, dialect: &Dialect) -> Result<String, Error> {
    if name.contains('.') {
        let separator = dialect.separator();
        return Err(Error::source(format!(
            "{} is not a usable key: `.` is not the nesting separator, `{separator}` is \
             (`auth{separator}jwt_secret` for `auth.jwt_secret`). Rename the entry, or move the \
             file out of the secrets directory.",
            path.display()
        )));
    }

    let spelled = dialect.env_spelling_of_name(name);
    if dialect.is_reserved(&spelled) {
        return Err(Error::source(format!(
            "{} names {spelled}, which is read directly from the environment before the layered \
             config is built, so a file cannot supply it.",
            path.display()
        )));
    }
    Ok(dialect.key_path(name))
}
