//! Whether a change to a key needs a restart: `FORMAT.md`'s ordered list, as a consumer applies it.
//!
//! A contract states two facts — whether the image rebuilds, and whether a rebuild applies each
//! key — and a consumer supplies the third: how its own deployment delivers the key. This module is
//! the combination, written once, so that `explain`, `diff`, the Helm generator and the gates can
//! never disagree about which keys a chart has to roll its pods for.
//!
//! # Every unknown is a restart
//!
//! An undeclared image, an unknown mode, an undeclared or unknown key class, a layer the image
//! does not say it watches: each resolves to [`Effective::Restart`], never to [`Effective::Live`].
//! That is the direction in which a mistake costs a needless rollout rather than a change that is
//! reported as applied and never is. And because a degraded answer must not look like a checked
//! one, the [`Why`] behind every restart is kept, and [`Why::degraded`] says which of them came
//! from something this build could not read.
//!
//! # What is not here
//!
//! Step 7 — a channel the orchestrator never updates in place — is a defect of the *deployment*,
//! not a class of the key, and `FORMAT.md` says to report it rather than classify it. It needs a
//! rendered manifest to see, so it lives with the gates that read one.

use serde_json::Value as Json;

use crate::document::{Contract, Key, Reload, ReloadLayer, ReloadMode, ReloadSupport};

/// How one key reaches a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    /// A variable: the key's own, an alias, or an indirection variable itself rather than the
    /// file it names.
    Environment,
    /// The TOML document.
    Document,
    /// A key-named file in the secrets directory.
    SecretsDir,
    /// The file an indirection variable names.
    EnvFile,
}

impl Channel {
    /// The layer an image watches for this channel, or [`None`] for the environment, which no
    /// image can watch.
    #[must_use]
    pub const fn layer(self) -> Option<ReloadLayer> {
        match self {
            Self::Environment => None,
            Self::Document => Some(ReloadLayer::Document),
            Self::SecretsDir => Some(ReloadLayer::SecretsDir),
            Self::EnvFile => Some(ReloadLayer::EnvFile),
        }
    }
}

/// What a change to one key, delivered one way, costs the running process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effective {
    /// The key arrives through the environment. Changing a variable changes the pod
    /// specification, and the orchestrator replaces the process without being asked.
    NotApplicable,
    /// A rebuild applies it, from the updated file, without a restart.
    Live,
    /// Only a process start applies it.
    Restart(Why),
}

impl Effective {
    /// Whether a deployment must restart the process for a change to this key.
    #[must_use]
    pub const fn needs_restart(self) -> bool {
        matches!(self, Self::Restart(_))
    }
}

/// Why a change needs a restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The container runs once per pod.
    InitContainer,
    /// The contract has no `schema.reload` — a document written before the field existed.
    Undeclared,
    /// The image declares that it applies nothing after start.
    NoRebuild,
    /// The image declares a mode this build does not know.
    UnknownMode,
    /// The key carries no `reload`.
    KeyUndeclared,
    /// The key is published `restart`.
    KeyRestart,
    /// The key carries a class this build does not know.
    UnknownClass,
    /// The image does not watch the layer the key arrives through.
    Unwatched(ReloadLayer),
}

impl Why {
    /// A short phrase for a report, completing "rolls the pods (…)".
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::InitContainer => "an init container reads it",
            Self::Undeclared => "the image declares nothing about reloading",
            Self::NoRebuild => "the image does not reload",
            Self::UnknownMode => "the image declares a reload mode this build does not know",
            Self::KeyUndeclared => "the key declares nothing about reloading",
            Self::KeyRestart => "only a restart applies it",
            Self::UnknownClass => "the key declares a reload class this build does not know",
            Self::Unwatched(_) => "the image does not watch the layer it arrives through",
        }
    }

    /// Whether this answer came from something this build could not read, and so has to be
    /// reported as degraded rather than as checked.
    #[must_use]
    pub const fn degraded(self) -> bool {
        matches!(self, Self::UnknownMode | Self::UnknownClass)
            || matches!(self, Self::Unwatched(ReloadLayer::Other))
    }
}

/// Steps 3 to 5: what the image says about `key`, before any channel is considered.
///
/// [`Ok`] means the image rebuilds and publishes the key `live`; whether the key is then live for
/// a given deployment depends on the channel, which [`classify`] adds.
///
/// # Errors
/// The [`Why`] of the first step that makes the key `restart`.
pub fn image_class(contract: &Contract, key: &Key) -> Result<(), Why> {
    class_of(contract.schema.reload.as_ref(), key.reload)
}

/// [`image_class`] over the two facts it reads, for a caller holding them apart.
///
/// # Errors
/// As [`image_class`].
pub fn class_of(support: Option<&ReloadSupport>, key: Option<Reload>) -> Result<(), Why> {
    let Some(support) = support else {
        return Err(Why::Undeclared);
    };
    match support.mode {
        ReloadMode::Rebuild => {}
        ReloadMode::None => return Err(Why::NoRebuild),
        ReloadMode::Other => return Err(Why::UnknownMode),
    }
    match key {
        Some(Reload::Live) => Ok(()),
        Some(Reload::Restart) => Err(Why::KeyRestart),
        Some(Reload::Other) => Err(Why::UnknownClass),
        None => Err(Why::KeyUndeclared),
    }
}

/// [`class_of`] over the raw JSON a caller that reads documents defensively holds.
///
/// A declaration that does not parse as one is read as a mode this build does not know, and a key
/// class that is not a string as a class it does not know: both are restarts, and both are
/// reported as degraded rather than as checked.
///
/// # Errors
/// As [`image_class`].
pub fn class_of_raw(support: Option<&Json>, key: Option<&Json>) -> Result<(), Why> {
    let support = match support.filter(|held| !held.is_null()) {
        None => None,
        Some(held) => Some(
            serde_json::from_value::<ReloadSupport>(held.clone()).map_err(|_| Why::UnknownMode)?,
        ),
    };
    let key = key
        .filter(|held| !held.is_null())
        .map(|held| serde_json::from_value::<Reload>(held.clone()).unwrap_or(Reload::Other));
    class_of(support.as_ref(), key)
}

/// Steps 1 to 6 for one key, delivered through `channel` to a container that is an init container
/// when `init` is set.
#[must_use]
pub fn classify(contract: &Contract, key: &Key, channel: Channel, init: bool) -> Effective {
    let Some(layer) = channel.layer() else {
        return Effective::NotApplicable;
    };
    if init {
        return Effective::Restart(Why::InitContainer);
    }
    if let Err(why) = image_class(contract, key) {
        return Effective::Restart(why);
    }
    let watched = contract
        .schema
        .reload
        .as_ref()
        .is_some_and(|support| support.layers.contains(&layer));
    if watched {
        Effective::Live
    } else {
        Effective::Restart(Why::Unwatched(layer))
    }
}

/// Whether the image rebuilds on a change at all — `mode: rebuild` with at least one layer.
#[must_use]
pub fn rebuilds(contract: &Contract) -> bool {
    contract
        .schema
        .reload
        .as_ref()
        .is_some_and(|support| support.mode == ReloadMode::Rebuild && !support.layers.is_empty())
}

/// What `contract` says about reloading that this build could not read, one line each.
///
/// Empty for every document this build fully understands. A consumer prints these beside whatever
/// it decided, because a restart that came from an unreadable value is a degraded answer and
/// `FORMAT.md` requires it to say so.
#[must_use]
pub fn degradations(contract: &Contract) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(support) = &contract.schema.reload {
        if support.mode == ReloadMode::Other {
            found.push(
                "`schema.reload.mode` is a mode this build does not know; every key is read as \
                 `restart`"
                    .to_owned(),
            );
        }
        if support.layers.contains(&ReloadLayer::Other) {
            found.push(
                "`schema.reload.layers` names a layer this build does not know; nothing is \
                 delivered through it here"
                    .to_owned(),
            );
        }
    }
    for key in &contract.schema.keys {
        if key.reload == Some(Reload::Other) {
            found.push(format!(
                "`{}` carries a reload class this build does not know, and is read as `restart`",
                key.path
            ));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Channel, Effective, Why, classify, degradations, rebuilds};
    use crate::document::Contract;

    fn contract(reload: &serde_json::Value, key_class: Option<&str>) -> Contract {
        let mut key = json!({
            "path": "ttl", "env": "APP_TTL", "env_file": "APP_TTL_FILE", "secrets_file": "ttl",
            "docs": "", "ty": "u64", "values": [], "text_form": "integer", "aliases": [],
            "env_aliases": [], "env_file_aliases": [], "secrets_file_aliases": [],
            "default": null, "default_value": null, "note": null,
            "required": false, "secret": false, "reserved": false
        });
        if let Some(class) = key_class {
            key["reload"] = json!(class);
        }
        let mut schema = json!({
            "schema_version": 2,
            "dialect": { "prefix": "APP_", "nesting_separator": "__", "indirection_suffix": "_FILE" },
            "loader": [],
            "keys": [key]
        });
        if !reload.is_null() {
            schema["reload"] = reload.clone();
        }
        serde_json::from_value(json!({
            "terrace_contract": 1,
            "producer": { "name": "test", "version": "0", "loader": "figment" },
            "app": { "name": "app" },
            "schema": schema,
            "json_schema": { "$schema": "http://json-schema.org/draft-07/schema#" },
            "external": { "env": [], "ignore": [], "unknown": "reject" }
        }))
        .expect("a well-formed contract")
    }

    fn rebuild() -> serde_json::Value {
        json!({ "mode": "rebuild", "layers": ["document", "secrets_dir"] })
    }

    fn of(contract: &Contract, channel: Channel) -> Effective {
        classify(contract, &contract.schema.keys[0], channel, false)
    }

    #[test]
    fn the_environment_is_never_a_reload_question() {
        let live = contract(&rebuild(), Some("live"));
        assert_eq!(of(&live, Channel::Environment), Effective::NotApplicable);
    }

    #[test]
    fn a_live_key_through_a_watched_layer_is_live() {
        let live = contract(&rebuild(), Some("live"));
        assert_eq!(of(&live, Channel::Document), Effective::Live);
        assert_eq!(of(&live, Channel::SecretsDir), Effective::Live);
        assert_eq!(
            of(&live, Channel::EnvFile),
            Effective::Restart(Why::Unwatched(crate::document::ReloadLayer::EnvFile))
        );
        assert_eq!(
            classify(&live, &live.schema.keys[0], Channel::Document, true),
            Effective::Restart(Why::InitContainer)
        );
    }

    #[test]
    fn every_unknown_is_a_restart_and_says_so() {
        let cases = [
            (
                contract(&serde_json::Value::Null, Some("live")),
                Why::Undeclared,
            ),
            (
                contract(&json!({ "mode": "none", "layers": [] }), None),
                Why::NoRebuild,
            ),
            (
                contract(
                    &json!({ "mode": "hot_swap", "layers": ["document"] }),
                    Some("live"),
                ),
                Why::UnknownMode,
            ),
            (contract(&rebuild(), None), Why::KeyUndeclared),
            (contract(&rebuild(), Some("restart")), Why::KeyRestart),
            (contract(&rebuild(), Some("eventually")), Why::UnknownClass),
        ];
        for (document, why) in cases {
            assert_eq!(of(&document, Channel::Document), Effective::Restart(why));
        }

        assert!(degradations(&contract(&rebuild(), Some("live"))).is_empty());
        assert_eq!(
            degradations(&contract(&rebuild(), Some("eventually"))).len(),
            1
        );
        assert!(!rebuilds(&contract(
            &json!({ "mode": "rebuild", "layers": [] }),
            Some("live")
        )));
    }
}
