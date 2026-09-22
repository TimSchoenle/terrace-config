//! Whether a configuration change reaches a running process, as the contract publishes it.
//!
//! Two facts, owned by two different places, because they are true of two different things:
//!
//! | Fact | Type | Stated by |
//! |---|---|---|
//! | Can this binary apply a change without restarting, and through which layers? | [`ReloadSupport`] | [`Terrace::reloads`](crate::Terrace::reloads) |
//! | Does a rebuild apply a change to this key? | [`Reload`] | `#[config(reload = "…")]`, through [`Sink::reload`](super::Sink::reload) |
//!
//! The first is a property of the *binary* rather than of the loader library: a service can link
//! the [`reload`](crate::reload) supervisor and never run it. The second is a property of the
//! *code* consuming each value: a key read inside the runtime a rebuild reconstructs is applied by
//! the rebuild, and one read before the supervisor starts — a `tracing` subscriber's filter, a
//! metrics recorder's endpoint — is not.
//!
//! A consumer derives the third fact, how its own deployment delivers each key, and combines the
//! three under `spec/v1/FORMAT.md`'s *Deciding whether a change needs a restart*. Every unknown
//! degrades to [`Reload::Restart`], which costs a needless restart rather than a change that is
//! silently never applied.

use serde::{Deserialize, Serialize};

/// Whether a rebuild applies a change to one key.
///
/// Published on [`Key::reload`](super::Key::reload). Absent means undeclared, which a consumer
/// reads as [`Self::Restart`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Reload {
    /// The value is consumed inside the runtime a rebuild reconstructs, so a rebuild applies it.
    Live,
    /// The value is applied only at process start.
    ///
    /// Either it is consumed before the supervisor runs, or changing it under live traffic is
    /// unsafe and the author wants a rollout's readiness gating and rollback instead. A rebuild
    /// keeps it at its boot value — see [`Terrace::reloader`](crate::Terrace::reloader).
    Restart,
    /// A class a later version of this crate names and this one does not.
    ///
    /// Read as [`Self::Restart`], the degradation target `FORMAT.md` sets. Kept distinct rather
    /// than folded in, so a reader can say it degraded instead of claiming the document said
    /// `restart`.
    #[serde(other)]
    Other,
}

impl Reload {
    /// Whether a rebuild applies a change to the key — `true` for [`Self::Live`] only.
    #[must_use]
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Live)
    }

    /// The spelling the contract publishes.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Restart => "restart",
            Self::Other => "other",
        }
    }
}

/// How a binary applies a configuration change after it has started, if at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReloadMode {
    /// Nothing is applied after start.
    None,
    /// A debounced change to a watched layer re-reads every layer and rebuilds the runtime,
    /// keeping every [`Reload::Restart`] key at its boot value.
    Rebuild,
    /// A mode a later version of this crate names and this one does not. Read as [`Self::None`].
    #[serde(other)]
    Other,
}

/// One file layer a rebuilding binary watches.
///
/// The environment is deliberately not a variant: a process's environment is fixed for its
/// lifetime, so no binary could truthfully claim to watch it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReloadLayer {
    /// The TOML layer at `<PREFIX>CONFIG`.
    Document,
    /// The directory of key-named files at `<PREFIX>SECRETS_DIR`.
    SecretsDir,
    /// The files `<PREFIX><KEY>_FILE` variables name.
    EnvFile,
    /// A layer a later version of this crate names and this one does not.
    #[serde(other)]
    Other,
}

/// Whether a binary applies a configuration change without restarting, and through which layers.
///
/// Published as `schema.reload`. Declared once, on the [`Terrace`](crate::Terrace) both the
/// contract generator and `main` construct:
///
/// ```
/// use terrace_config::Terrace;
/// use terrace_config::schema::ReloadSupport;
///
/// fn layers() -> Terrace {
///     Terrace::new("MYAPP_").reloads(ReloadSupport::rebuild())
/// }
/// # let _ = layers();
/// ```
///
/// Without a declaration nothing is published, which a consumer reads as undeclared and treats as
/// [`ReloadMode::None`]. That keeps every contract written before this existed byte-identical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ReloadSupport {
    /// How a change is applied.
    pub mode: ReloadMode,
    /// The layers watched for one. Empty exactly when [`Self::mode`] is [`ReloadMode::None`].
    pub layers: Vec<ReloadLayer>,
}

impl ReloadSupport {
    /// The binary applies nothing after start.
    ///
    /// Every key is then published [`Reload::Restart`], whatever its attributes say: a type
    /// annotated `live` is legitimately shared with a binary that does not reload, and the
    /// contract describes this binary.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            mode: ReloadMode::None,
            layers: Vec::new(),
        }
    }

    /// The binary runs [`reload::run`](crate::reload) over what
    /// [`Terrace::reloader`](crate::Terrace::reloader) loads, which watches all three file layers.
    #[must_use]
    pub fn rebuild() -> Self {
        Self {
            mode: ReloadMode::Rebuild,
            layers: vec![
                ReloadLayer::Document,
                ReloadLayer::SecretsDir,
                ReloadLayer::EnvFile,
            ],
        }
    }

    /// Whether a change can be applied after start at all.
    #[must_use]
    pub fn rebuilds(&self) -> bool {
        self.mode == ReloadMode::Rebuild && !self.layers.is_empty()
    }
}

/// The reload class a key under the scopes currently open resolves to.
///
/// **An explicit `restart` anywhere on the path wins; otherwise an explicit `live`; otherwise
/// undeclared.** The order a field's attribute and a nested type's own attribute are opened in
/// cannot matter, because either order would let a `live` somewhere silently override a `restart`
/// somewhere else — and that is the direction in which a change is never applied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Scopes {
    /// How many `live` scopes are open.
    live: usize,
    /// How many `restart` scopes — or scopes of a class this build does not know — are open.
    restart: usize,
}

impl Scopes {
    /// Open one scope of `class`.
    pub(super) fn open(&mut self, class: Reload) {
        if class.is_live() {
            self.live += 1;
        } else {
            self.restart += 1;
        }
    }

    /// Close the scope [`Self::open`] opened for `class`.
    pub(super) fn close(&mut self, class: Reload) {
        if class.is_live() {
            self.live -= 1;
        } else {
            self.restart -= 1;
        }
    }

    /// The class a key recorded now takes, or [`None`] when nothing on its path said.
    pub(super) const fn resolve(self) -> Option<Reload> {
        if self.restart > 0 {
            Some(Reload::Restart)
        } else if self.live > 0 {
            Some(Reload::Live)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Reload, ReloadSupport, Scopes};

    #[test]
    fn restart_wins_in_either_order() {
        for order in [
            [Reload::Live, Reload::Restart],
            [Reload::Restart, Reload::Live],
        ] {
            let mut scopes = Scopes::default();
            for class in order {
                scopes.open(class);
            }
            assert_eq!(scopes.resolve(), Some(Reload::Restart));
        }
    }

    #[test]
    fn an_unknown_class_counts_as_restart() {
        let mut scopes = Scopes::default();
        scopes.open(Reload::Live);
        scopes.open(Reload::Other);
        assert_eq!(scopes.resolve(), Some(Reload::Restart));
        scopes.close(Reload::Other);
        assert_eq!(scopes.resolve(), Some(Reload::Live));
        scopes.close(Reload::Live);
        assert_eq!(scopes.resolve(), None);
    }

    #[test]
    fn only_a_rebuild_with_layers_rebuilds() {
        assert!(ReloadSupport::rebuild().rebuilds());
        assert!(!ReloadSupport::none().rebuilds());
    }
}
