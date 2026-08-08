//! What a load produced, and what a reload needs in order to do it again.

use std::path::PathBuf;

/// A loaded config together with what a reload needs to watch and to compare against.
#[derive(Debug, Clone)]
pub struct Loaded<T> {
    /// The extracted config.
    pub value: T,
    /// Where it came from.
    pub sources: Sources,
}

/// The filesystem inputs a config was assembled from, and a fingerprint of the result.
///
/// The fingerprint is the fully merged figment value rather than the typed config: config
/// structs that hold secrets typically hold a type such as `secrecy::SecretString`, which
/// deliberately has no `PartialEq`, so the typed value cannot be compared. Comparing the merged
/// value instead means a reload that changes nothing — a `ConfigMap` rewritten with identical
/// contents, a `..data` swap that moved no key — is detected as a no-op before anything is torn
/// down and rebuilt.
///
/// That fingerprint contains **every configuration value, secrets included**, which is why
/// [`Debug`] is written by hand and redacts it. Printing a `Sources` must never be a way to
/// print a credential.
#[derive(Clone)]
pub struct Sources {
    /// Directories to watch, sorted and deduplicated.
    pub(crate) watch: Vec<PathBuf>,
    /// The fully merged value, for change detection only.
    pub(crate) fingerprint: figment::value::Value,
}

impl Sources {
    /// Directories to watch for changes.
    ///
    /// Directories, not files: a Kubernetes volume update renames a whole new `..data`
    /// directory over the old one, so a watch registered against a file's inode never fires a
    /// second time.
    #[must_use]
    pub fn watch_paths(&self) -> &[PathBuf] {
        &self.watch
    }

    /// Whether `self` resolves to different values than `previous`.
    #[must_use]
    pub fn differs_from(&self, previous: &Self) -> bool {
        self.fingerprint != previous.fingerprint
    }
}

impl std::fmt::Debug for Sources {
    /// The watch paths, never the fingerprint. See the type's own documentation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sources")
            .field("watch", &self.watch)
            .field("fingerprint", &"<redacted>")
            .finish()
    }
}

#[cfg(feature = "reload")]
impl crate::reload::Source for Sources {
    fn watch_paths(&self) -> &[PathBuf] {
        Self::watch_paths(self)
    }

    fn differs_from(&self, previous: &Self) -> bool {
        Self::differs_from(self, previous)
    }
}
