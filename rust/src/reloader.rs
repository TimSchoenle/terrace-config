//! Loading a configuration for a supervisor that keeps restart-class keys at their boot values.
//!
//! [`reload::run`](crate::reload) rebuilds the whole runtime from whatever the reload closure
//! returns. Left at that, every key is applied by every rebuild, and a key published
//! [`Reload::Restart`](crate::schema::Reload::Restart) — one consumed before the supervisor runs,
//! or one the author does not want changed under live traffic — changes the moment any `live` key
//! beside it does. The contract would then say something the process does not do.
//!
//! [`Reloader`] closes that gap by **pinning**: every restart-class path, and every alias of it,
//! keeps the value it had when the process started. A change confined to those keys is therefore
//! no change at all to the running service, and is reported as a pending restart instead — see
//! [`Sources::pending_restart`].
//!
//! The restart paths come from the same [`Describe`] implementation the contract is generated
//! from, so the runtime and the published document cannot disagree about which keys they are.

use std::marker::PhantomData;
use std::sync::Arc;

use figment::Figment;
use figment::providers::Serialized;
use figment::value::Value;
use serde::de::DeserializeOwned;

use crate::error::Error;
use crate::loaded::{Loaded, Sources, same_value};
use crate::schema::Describe;
use crate::terrace::Terrace;

/// Re-loads a configuration with every restart-class key held at its boot value.
///
/// Built by [`Terrace::reloader`], alongside the boot load it captured the values from. Hand
/// [`Self::reload`] to [`reload::run`](crate::reload) as the reload closure:
///
/// ```no_run
/// # #[cfg(feature = "reload")]
/// # async fn serve(
/// #     _: std::sync::Arc<Config>,
/// #     _: tokio_util::sync::CancellationToken,
/// # ) -> Result<(), ServiceError> { Ok(()) }
/// # #[derive(Debug, thiserror::Error)]
/// # enum ServiceError {
/// #     #[error("{0}")] Config(#[from] terrace_config::Error),
/// #     #[cfg(feature = "reload")]
/// #     #[error("{0}")] Watch(#[from] terrace_config::reload::WatchError),
/// # }
/// use terrace_config::Terrace;
/// use terrace_config::schema::{Describe, ReloadSupport};
///
/// #[derive(serde::Deserialize, Describe)]
/// #[config(reload = "live")]
/// struct Config {
///     /// Seconds a rendered page stays cached.
///     #[serde(default)]
///     ttl_secs: u64,
/// }
///
/// fn layers() -> Terrace {
///     Terrace::new("MYAPP_").reloads(ReloadSupport::rebuild())
/// }
///
/// # #[cfg(feature = "reload")]
/// # async fn run() -> Result<(), ServiceError> {
/// let (boot, reloader) = layers().reloader::<Config>()?;
/// let shutdown = tokio_util::sync::CancellationToken::new();
/// terrace_config::reload::run(
///     boot.into(),
///     &shutdown,
///     || reloader.reload().map(Into::into).map_err(ServiceError::from),
///     serve,
/// )
/// .await
/// # }
/// ```
///
/// The boot values are captured once and never replaced. Pinning to the previous *generation*
/// instead would let a restart key drift one reload at a time — each rebuild carrying forward a
/// value the process was never started with.
pub struct Reloader<T> {
    /// The loader to re-read through, as it was when the reloader was built.
    terrace: Terrace,
    /// Every restart-class path, and its value at boot.
    pins: Arc<Pins>,
    /// A reloader produces `T`s and holds none.
    _config: PhantomData<fn() -> T>,
}

impl<T> std::fmt::Debug for Reloader<T> {
    /// The pinned paths, never the pinned values.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reloader")
            .field("terrace", &self.terrace)
            .field("pins", &self.pins)
            .finish()
    }
}

impl<T: DeserializeOwned> Reloader<T> {
    /// Load the configuration again, with every restart-class key held at its boot value.
    ///
    /// Reads the environment and the files exactly as the boot load did. The typed value is
    /// extracted from the configuration **as it is on disk** first, so a restart key whose new
    /// value would fail the next boot fails this reload now, loudly, rather than at the next
    /// restart — and only then from the pinned one, which is what the rebuilt runtime receives.
    ///
    /// # Errors
    /// As [`Terrace::load`]: a value that fails to parse, a file-backed source that cannot be
    /// read, a key supplied twice. [`reload::run`](crate::reload) keeps the running service on
    /// any of them.
    pub fn reload(&self) -> Result<Loaded<T>, Error> {
        let (figment, watch) = self.terrace.assemble_watched()?;
        let on_disk = figment.extract::<Value>().map_err(Box::new)?;
        let value = figment.extract::<T>().map_err(Box::new)?;

        let pending = self.pins.pending(&on_disk);
        if pending.is_empty() {
            return Ok(Loaded {
                value,
                sources: Sources::new(watch, on_disk).with_pending(pending),
            });
        }

        let pinned = self.pins.apply(on_disk);
        let value = Figment::from(Serialized::defaults(&pinned))
            .extract::<T>()
            .map_err(Box::new)?;
        Ok(Loaded {
            value,
            sources: Sources::new(watch, pinned).with_pending(pending),
        })
    }
}

impl Terrace {
    /// Load a typed config for a supervisor, and the [`Reloader`] that loads it again.
    ///
    /// The one entry point for a binary that declared
    /// [`ReloadSupport::rebuild`](crate::schema::ReloadSupport::rebuild): the boot load captures
    /// the value of every key `T` does not describe as [`Reload::Live`](crate::schema::Reload),
    /// and the reloader holds them there for the life of the process. See [`Reloader`].
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] when this loader did not declare a rebuild — a binary publishing
    /// `none` has no business supervising reloads — and otherwise as [`Terrace::load`].
    ///
    /// # Panics
    /// As [`Terrace::schema`]: if two of `T`'s fields resolve to one key path, or `T` contains
    /// itself.
    pub fn reloader<T: DeserializeOwned + Describe>(
        &self,
    ) -> Result<(Loaded<T>, Reloader<T>), Error> {
        if !self
            .reload_support()
            .is_some_and(crate::schema::ReloadSupport::rebuilds)
        {
            return Err(Error::Invalid(
                "`Terrace::reloader` is for a binary that rebuilds on a change, and this loader \
                 declares no rebuild. Declare it with `Terrace::reloads(ReloadSupport::rebuild())`, \
                 which is also what the published contract states."
                    .to_owned(),
            ));
        }
        let schema = self.schema::<T>();

        let (figment, watch) = self.assemble_watched()?;
        let on_disk = figment.extract::<Value>().map_err(Box::new)?;
        let value = figment.extract::<T>().map_err(Box::new)?;

        let pins = Arc::new(Pins::capture(&schema.restart_paths(), &on_disk));
        Ok((
            Loaded {
                value,
                sources: Sources::new(watch, on_disk),
            },
            Reloader {
                terrace: self.clone(),
                pins,
                _config: PhantomData,
            },
        ))
    }
}

/// Every restart-class path, and what it held at boot — [`None`] where it was absent.
struct Pins {
    /// Sorted by path, so a pending report comes out in a stable order.
    entries: Vec<(String, Option<Value>)>,
}

impl std::fmt::Debug for Pins {
    /// The paths, never the values: a pinned value is a configuration value, secrets included.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.entries.iter().map(|(path, _)| path))
            .finish()
    }
}

impl Pins {
    /// Record what each of `paths` holds in `boot`.
    fn capture(paths: &[&str], boot: &Value) -> Self {
        let mut entries: Vec<(String, Option<Value>)> = paths
            .iter()
            .map(|path| ((*path).to_owned(), boot.find_ref(path).cloned()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries.dedup_by(|a, b| a.0 == b.0);
        Self { entries }
    }

    /// The pinned paths whose value in `on_disk` is not the one they had at boot.
    fn pending(&self, on_disk: &Value) -> Vec<String> {
        self.entries
            .iter()
            .filter(
                |(path, boot)| match (boot.as_ref(), on_disk.find_ref(path)) {
                    (None, None) => false,
                    (Some(boot), Some(now)) => !same_value(boot, now),
                    _ => true,
                },
            )
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// `on_disk` with every pinned path set back to its boot state, absent included.
    fn apply(&self, mut on_disk: Value) -> Value {
        for (path, boot) in &self.entries {
            set_path(&mut on_disk, path, boot.clone());
        }
        on_disk
    }
}

/// Set the node at the dotted `path` to `value`, or remove it when `value` is [`None`].
///
/// Tables on the way are created when setting and left alone when removing, so restoring a key
/// that was absent at boot never invents an empty table the boot configuration did not have. A
/// path running through something that is not a table is left untouched: the boot value cannot
/// have lived there either, because a figment value is a tree.
fn set_path(root: &mut Value, path: &str, value: Option<Value>) {
    let segments: Vec<&str> = path.split('.').collect();
    let Some((last, parents)) = segments.split_last() else {
        return;
    };

    let mut node = root;
    for segment in parents {
        let Value::Dict(tag, dict) = node else {
            return;
        };
        if value.is_none() && !dict.contains_key(*segment) {
            return;
        }
        let tag = *tag;
        node = dict
            .entry((*segment).to_owned())
            .or_insert_with(|| Value::Dict(tag, figment::value::Dict::new()));
    }

    let Value::Dict(_, dict) = node else {
        return;
    };
    match value {
        Some(value) => {
            dict.insert((*last).to_owned(), value);
        }
        None => {
            dict.remove(*last);
        }
    }
}

#[cfg(test)]
mod tests {
    use figment::value::Value;
    use figment::{Figment, providers::Serialized};

    use super::{Pins, set_path};

    fn value(json: &str) -> Value {
        let parsed: serde_json::Value = serde_json::from_str(json).expect("test json");
        Figment::from(Serialized::defaults(parsed))
            .extract()
            .expect("test value")
    }

    #[test]
    fn a_change_to_a_pinned_key_is_pending_and_undone() {
        let boot = value(r#"{"log": {"level": "info"}, "ttl": 1}"#);
        let pins = Pins::capture(&["log.level"], &boot);

        let now = value(r#"{"log": {"level": "debug"}, "ttl": 2}"#);
        assert_eq!(pins.pending(&now), ["log.level"]);

        let pinned = pins.apply(now);
        assert_eq!(
            pinned.find_ref("log.level").and_then(Value::as_str),
            Some("info")
        );
        assert_eq!(pinned.find_ref("ttl").and_then(Value::to_u128), Some(2));
    }

    #[test]
    fn a_key_absent_at_boot_stays_absent() {
        let boot = value(r#"{"ttl": 1}"#);
        let pins = Pins::capture(&["log.level"], &boot);

        let now = value(r#"{"log": {"level": "debug"}, "ttl": 1}"#);
        assert_eq!(pins.pending(&now), ["log.level"]);
        let pinned = pins.apply(now);
        assert!(pinned.find_ref("log.level").is_none());
        // The table the new value arrived in is left as it was: only the pinned key is undone.
        assert!(pinned.find_ref("log").is_some());
    }

    #[test]
    fn nothing_changed_is_nothing_pending() {
        let boot = value(r#"{"log": {"level": "info"}}"#);
        let pins = Pins::capture(&["log.level", "absent.key"], &boot);
        assert!(pins.pending(&boot).is_empty());
    }

    #[test]
    fn restoring_a_value_creates_the_tables_it_lived_in() {
        let mut root = value("{}");
        set_path(&mut root, "a.b.c", Some(Value::from(1u8)));
        assert_eq!(root.find_ref("a.b.c").and_then(Value::to_u128), Some(1));
    }

    #[test]
    fn the_debug_form_names_paths_and_never_values() {
        let boot = value(r#"{"token": "hunter2"}"#);
        let pins = Pins::capture(&["token"], &boot);
        let printed = format!("{pins:?}");
        assert!(printed.contains("token"));
        assert!(!printed.contains("hunter2"));
    }
}
