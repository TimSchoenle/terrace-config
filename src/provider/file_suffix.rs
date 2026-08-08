//! Per-key file indirection: `MYAPP_<KEY>_FILE=/path`, which is what Docker Compose
//! `secrets:` and a number of official images look like.

use std::collections::BTreeMap;
use std::path::PathBuf;

use figment::value::{Dict, Map, Value};
use figment::{Metadata, Profile, Provider};

use crate::dialect::Dialect;
use crate::error::Error;
use crate::provider::{FileValue, insert_nested, read_value};

/// Every `<PREFIX><KEY><SUFFIX>` indirection the environment declares, read at construction.
#[derive(Debug, Clone)]
pub struct FileSuffixEnv {
    /// The values found, keyed by figment key path.
    values: BTreeMap<String, FileValue>,
}

impl FileSuffixEnv {
    /// Read every indirection variable `dialect` recognises.
    ///
    /// Scanned over the whole environment rather than by `env::var("…")` on a literal, because
    /// the keys are open-ended: there is no list of them to consult.
    ///
    /// # Errors
    /// Returns [`Error::Source`] if a named path cannot be read or is not UTF-8, or if a
    /// variable names a key reserved by `dialect`.
    pub fn read(dialect: &Dialect) -> Result<Self, Error> {
        let prefix = dialect.prefix();
        let suffix = dialect.indirection_suffix();

        let mut values = BTreeMap::new();
        for (name, path) in std::env::vars_os() {
            let (Some(name), Some(path)) = (name.to_str(), path.to_str()) else {
                continue;
            };
            let Some(key) = name
                .strip_prefix(prefix)
                .and_then(|k| k.strip_suffix(suffix))
            else {
                continue;
            };
            if key.is_empty() {
                continue;
            }

            let spelled = format!("{prefix}{key}");
            if dialect.is_reserved(&spelled) {
                return Err(Error::source(format!(
                    "{name} is set, but {spelled} is read directly from the environment before \
                     the layered config is built, so a file cannot supply it. Set {spelled} \
                     itself."
                )));
            }

            let path = PathBuf::from(path);
            // A `_FILE` naming an unreadable path is fatal rather than skipped. Skipping is how
            // a secret goes silently unset and the service boots with a default instead.
            let value = read_value(&path)
                .map_err(|e| Error::source(format!("{name} names {}: {e}", path.display())))?;
            values.insert(dialect.key_path(key), FileValue { path, value });
        }

        Ok(Self { values })
    }

    /// The values this layer supplies, keyed by figment key path (`auth.jwt_secret`).
    #[must_use]
    pub fn values(&self) -> &BTreeMap<String, FileValue> {
        &self.values
    }

    /// Whether the environment declared no indirection variables.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The directories a reload has to watch: the parent of every named path.
    ///
    /// Parent directories rather than the files themselves — a Kubernetes volume update
    /// replaces the file by renaming a new `..data` directory over the old one, so a watch
    /// registered against the old inode never fires again.
    pub(crate) fn watch_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.values
            .values()
            .filter_map(|value| value.path.parent().map(std::path::Path::to_path_buf))
    }
}

impl Provider for FileSuffixEnv {
    fn metadata(&self) -> Metadata {
        Metadata::named("file-indirection environment variables")
    }

    fn data(&self) -> Result<Map<Profile, Dict>, figment::Error> {
        let mut dict = Dict::new();
        for (key, value) in &self.values {
            insert_nested(&mut dict, key, Value::from(value.value.as_str()));
        }
        Ok(Profile::Default.collect(dict))
    }
}
