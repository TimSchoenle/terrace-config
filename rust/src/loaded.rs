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

impl<T> From<Loaded<T>> for (T, Sources) {
    /// The pair [`reload::run`](crate::reload) takes as its boot value and its reload result.
    fn from(loaded: Loaded<T>) -> Self {
        (loaded.value, loaded.sources)
    }
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
    /// The restart-class paths whose value on disk differs from the one the process was started
    /// with. Always empty outside a pinning reload.
    pub(crate) pending: Vec<String>,
}

impl Sources {
    /// Sources with nothing pending.
    pub(crate) fn new(watch: Vec<PathBuf>, fingerprint: figment::value::Value) -> Self {
        Self {
            watch,
            fingerprint,
            pending: Vec::new(),
        }
    }

    /// These sources, recording `pending` as the restart-class paths changed on disk.
    #[cfg_attr(
        not(feature = "schema"),
        expect(
            dead_code,
            reason = "only the pinning reloader records a pending restart"
        )
    )]
    pub(crate) fn with_pending(mut self, pending: Vec<String>) -> Self {
        self.pending = pending;
        self
    }

    /// The keys whose value on disk differs from the one the process is running with, because a
    /// rebuild does not apply them.
    ///
    /// Filled in only by [`Terrace::reloader`](crate::Terrace::reloader)'s reloads, which keep
    /// every restart-class key at its boot value; empty everywhere else. Paths, never values, and
    /// sorted. A non-empty list is real state an operator needs to see — the file says one thing
    /// and the process does another until it restarts — which is why
    /// [`reload::run`](crate::reload) logs every change to it.
    #[must_use]
    pub fn pending_restart(&self) -> &[String] {
        &self.pending
    }

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
    ///
    /// Compared structurally rather than with `==`, for one reason: `NaN != NaN`. A configuration
    /// holding a float NaN — `timeout = nan` in a TOML file — made a fingerprint unequal to
    /// *itself*, so every filesystem event looked like a change and the supervisor tore the
    /// service down and rebuilt it, for as long as that key stayed in the file. Found by the
    /// `toml_layers` fuzz target.
    ///
    /// Floats therefore compare by their bits, which makes the relation reflexive. It also makes
    /// `0.0` and `-0.0` distinct, which is the safe direction: the cost is one needless reload of
    /// a configuration nobody writes, against a reload loop that never ends.
    #[must_use]
    pub fn differs_from(&self, previous: &Self) -> bool {
        !same_value(&self.fingerprint, &previous.fingerprint)
    }
}

impl std::fmt::Debug for Sources {
    /// The watch paths, never the fingerprint.
    ///
    /// See the type's own documentation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sources")
            .field("watch", &self.watch)
            .field("fingerprint", &"<redacted>")
            .field("pending", &self.pending)
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

    fn pending_restart(&self) -> &[String] {
        Self::pending_restart(self)
    }
}

/// Structural equality over a merged configuration value, with floats compared by their bits.
///
/// Mirrors figment's own `PartialEq for Value` — which ignores the [`Tag`] on every variant, since
/// two loads of one file are the same configuration however they were provided — and changes
/// exactly one thing: a float leaf compares by bits, so a value equals itself. See
/// [`Sources::differs_from`].
///
/// [`Tag`]: figment::value::Tag
pub(crate) fn same_value(a: &figment::value::Value, b: &figment::value::Value) -> bool {
    use figment::value::Value;

    match (a, b) {
        (Value::Num(_, left), Value::Num(_, right)) => same_num(left, right),
        (Value::Dict(_, left), Value::Dict(_, right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|((left_key, left), (right_key, right))| {
                        left_key == right_key && same_value(left, right)
                    })
        }
        (Value::Array(_, left), Value::Array(_, right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| same_value(left, right))
        }
        _ => a == b,
    }
}

/// Two numbers, where the float variants compare by bits so a value equals itself.
fn same_num(a: &figment::value::Num, b: &figment::value::Num) -> bool {
    use figment::value::Num;

    match (a, b) {
        (Num::F32(left), Num::F32(right)) => left.to_bits() == right.to_bits(),
        (Num::F64(left), Num::F64(right)) => left.to_bits() == right.to_bits(),
        _ => a == b,
    }
}
