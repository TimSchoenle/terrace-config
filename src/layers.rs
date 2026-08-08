//! What happens when the file-backed layers meet each other and the plain environment.
//!
//! Each provider in [`crate::provider`] is correct on its own. The question this module answers
//! is the one none of them can see: whether the *same key* arrived through more than one
//! mechanism, and what to do about it.

use std::collections::BTreeSet;
use std::path::PathBuf;

use figment::value::{Dict, Map, Value};
use figment::{Metadata, Profile, Provider};

use crate::dialect::Dialect;
use crate::error::Error;
use crate::provider::{FileSuffixEnv, SecretsDir, insert_nested};

/// What to do when one key is supplied by two mechanisms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadowPolicy {
    /// Refuse to load. **The default, and the reason this crate exists.**
    ///
    /// Precedence would be the softer option, and it is the wrong one here. The failure this
    /// prevents is a half-migrated deployment where a stale environment variable shadows a
    /// mounted secret that has since been rotated — the service keeps working, with the old
    /// credential, and the discrepancy surfaces during an incident rather than during a deploy.
    #[default]
    Reject,
    /// Resolve by precedence: the environment first, then the secrets directory, then `_FILE`
    /// indirection, each overwriting the last.
    ///
    /// Exists so the crate is adoptable rather than because it is a good idea. Anyone migrating
    /// from `figment_file_provider_adapter` has precedence semantics today and will not switch
    /// to a crate that fails their boot on the first deploy.
    LastWins,
}

/// The file-backed layers, collected together so the shadowing check can see all of them at
/// once.
#[derive(Debug, Clone, Default)]
pub(crate) struct FileLayers {
    /// Values from the secrets directory, when one was configured.
    secrets: Option<SecretsDir>,
    /// Values from `<PREFIX><KEY><SUFFIX>` indirection.
    files: Option<FileSuffixEnv>,
}

impl FileLayers {
    /// Read every file-backed layer the environment points at, and apply `policy`.
    ///
    /// `dir` is the secrets directory, when one is configured; `origin` names whatever pointed
    /// at it.
    ///
    /// # Errors
    /// Returns [`Error::Source`] if a configured secrets directory or indirection path cannot
    /// be read, if a file's contents are not UTF-8, if a file name is not a usable key, or —
    /// under [`ShadowPolicy::Reject`] — if one key is supplied by more than one mechanism.
    pub(crate) fn collect(
        dir: Option<PathBuf>,
        origin: &str,
        dialect: &Dialect,
        policy: ShadowPolicy,
    ) -> Result<Self, Error> {
        let layers = Self {
            secrets: dir
                .map(|dir| SecretsDir::read(origin, dir, dialect))
                .transpose()?,
            files: Some(FileSuffixEnv::read(dialect)?),
        };
        if policy == ShadowPolicy::Reject {
            layers.reject_shadowed_keys(dialect)?;
        }
        Ok(layers)
    }

    /// Whether any file-backed value was found. Callers skip the provider entirely when not.
    pub(crate) fn is_empty(&self) -> bool {
        self.secrets.as_ref().is_none_or(SecretsDir::is_empty)
            && self.files.as_ref().is_none_or(FileSuffixEnv::is_empty)
    }

    /// The paths a reload has to watch: the secrets directory, and every indirection target's
    /// parent.
    ///
    /// Directories, not files: a Kubernetes volume update renames a whole new `..data`
    /// directory over the old one, so a watch registered against a file's inode never fires a
    /// second time.
    pub(crate) fn watch_paths(&self) -> BTreeSet<PathBuf> {
        let mut paths = BTreeSet::new();
        if let Some(secrets) = &self.secrets {
            paths.insert(secrets.dir().to_path_buf());
        }
        if let Some(files) = &self.files {
            paths.extend(files.watch_paths());
        }
        paths
    }

    /// Refuse a key supplied by more than one of: the environment, the secrets directory, the
    /// indirection variables.
    fn reject_shadowed_keys(&self, dialect: &Dialect) -> Result<(), Error> {
        let env = dialect.plain_env_keys();
        let empty = std::collections::BTreeMap::default();
        let secrets = self.secrets.as_ref().map_or(&empty, SecretsDir::values);
        let files = self.files.as_ref().map_or(&empty, FileSuffixEnv::values);

        for (key, value) in secrets {
            if let Some(other) = files.get(key) {
                return Err(shadowed(
                    key,
                    &value.path().display(),
                    &other.path().display(),
                ));
            }
            if env.contains(key) {
                return Err(shadowed(
                    key,
                    &value.path().display(),
                    &dialect.env_spelling(key),
                ));
            }
        }
        for (key, value) in files {
            if env.contains(key) {
                return Err(shadowed(
                    key,
                    &value.path().display(),
                    &dialect.env_spelling(key),
                ));
            }
        }
        Ok(())
    }
}

impl Provider for FileLayers {
    fn metadata(&self) -> Metadata {
        Metadata::named("file-backed configuration")
    }

    fn data(&self) -> Result<Map<Profile, Dict>, figment::Error> {
        let mut dict = Dict::new();
        // The secrets directory first so indirection wins on the merge. Under
        // `ShadowPolicy::Reject` they cannot actually collide — `reject_shadowed_keys` already
        // refused that — but the ordering makes the provider correct on its own rather than
        // only in the context that validates it, and it is what `LastWins` means.
        let secrets = self.secrets.iter().flat_map(SecretsDir::values);
        let files = self.files.iter().flat_map(FileSuffixEnv::values);
        for (key, value) in secrets.chain(files) {
            insert_nested(&mut dict, key, Value::from(value.value()));
        }
        Ok(Profile::Default.collect(dict))
    }
}

/// The one shadowing error, so every direction reads identically.
fn shadowed(key: &str, source: &impl std::fmt::Display, other: &impl std::fmt::Display) -> Error {
    Error::source(format!(
        "`{key}` is supplied twice — by {source} and by {other}. Remove one: a stale \
         environment variable shadowing a rotated secret keeps the service running on the old \
         credential."
    ))
}
