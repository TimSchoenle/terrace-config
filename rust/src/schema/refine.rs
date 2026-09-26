//! Constraints a type cannot state, supplied when the schema is built.
//!
//! [`#[derive(Describe)]`](macro@super::Describe) reads a key's constraint off its Rust type, and
//! some constraints are real without being in any type. A legal-pages library holds its documents
//! as a `BTreeMap<String, LegalDocument>`; the application mounting it refuses to start unless the
//! map holds `imprint` and `privacy`. The type says "a map of documents", the contract publishes
//! `{}` as a valid default, and a chart that renders exactly that passes every gate and fails at
//! boot.
//!
//! [`Schema::refine`] is how the application says so, and [`Refine`] is how a library that owns the
//! runtime check publishes it without knowing where it is mounted. Both go through one rule: **a
//! refinement tightens and never loosens.** It is a typed enum of tightenings rather than a JSON
//! Schema fragment for exactly that reason — a fragment can say anything, including something that
//! widens what the type stated, and nothing downstream could tell.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde_json::{Map, Value as Json};

use crate::dialect::Dialect;
use crate::error::Error;

use super::check::{self, Verdict};
use super::{
    Key, LATEST_SCHEMA_VERSION, Schema, Unreachable, env_layer_key, env_spelling, pattern,
    secrets_file_name,
};

/// One tightening of a key's constraint, beyond what its type states.
///
/// `#[non_exhaustive]` because the set grows, and every variant has to meet the bar the first one
/// does: it tightens, it is checkable by [`Schema::refine`] before it is published, and it has a
/// meaning every rendering can show.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refinement {
    /// A map-typed key must contain these entries, whatever else it holds.
    ///
    /// Published as JSON Schema's `required` inside the key's
    /// [`constraint`](super::Key::constraint), unioned with any entries already required. The map
    /// stays open: an entry not named here is still accepted, exactly as the type accepts it.
    ///
    /// An empty set is **accepted and changes nothing**, once the path and the key's shape have
    /// been checked. A library computing its required entries at runtime can legitimately compute
    /// none, and refusing that would make every caller special-case it — while skipping the checks
    /// for an empty set would let a mistyped path through exactly when the set happens to be empty.
    RequiredEntries(BTreeSet<String>),
    /// Every entry name of a map-typed key must match this pattern.
    ///
    /// Published as JSON Schema's `propertyNames: {"pattern": …}` inside the key's
    /// [`constraint`](super::Key::constraint). The pattern is matched as JSON Schema matches one —
    /// unanchored, so a pattern meaning the whole name says so with `^` and `$` — and it must lie
    /// inside the *portable subset*: the constructs every engine a contract is read by agrees on.
    /// [`Schema::refine`] refuses anything outside it, naming the construct and the portable
    /// spelling. The subset is set out in `spec/v1/FORMAT.md`, under *Portable patterns*; in short,
    /// no `.`, no `\d`, `\w`, `\s` or `\b`, no lookaround and no lazy repetition — a class naming
    /// exactly the characters meant says the same thing in every engine.
    ///
    /// **The environment folds names to lower case.** A pattern admitting an upper-case name
    /// describes an entry no environment variable can supply, since `PORTFOLIO_LEGAL__DOCUMENTS__TERMS`
    /// arrives as `terms` whatever case it was written in. That is weaker than the loader, not wrong
    /// — the entry is still supplied from a file — so it is published as written; a pattern meant to
    /// describe what every layer can supply names lower-case characters only.
    EntryNames(String),
}

impl Refinement {
    /// [`Self::RequiredEntries`] from any list of names.
    #[must_use]
    pub fn required_entries<I, S>(entries: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::RequiredEntries(entries.into_iter().map(Into::into).collect())
    }

    /// [`Self::EntryNames`] from a pattern.
    ///
    /// Checked when it is applied, by [`Schema::refine`], where a refusal can name the key it was
    /// meant for.
    #[must_use]
    pub fn entry_names(pattern: impl Into<String>) -> Self {
        Self::EntryNames(pattern.into())
    }

    /// The version of the schema half a document carrying this refinement is published at.
    const fn schema_version(&self) -> u32 {
        match self {
            Self::RequiredEntries(_) => super::SCHEMA_VERSION,
            Self::EntryNames(_) => LATEST_SCHEMA_VERSION,
        }
    }
}

/// A source of refinements — a library whose runtime validation should be published.
///
/// Implemented by whatever owns the check the type cannot state, so the contract and the check
/// cannot drift: the library that refuses to start without an `imprint` document is the one that
/// says so here. Paths are **relative to where the library's configuration is mounted**, which
/// only the host knows — [`Schema::refine_with`] takes that mount point, so a library never
/// hard-codes it.
///
/// A relative path may be empty, meaning the mount point itself: a library whose whole
/// configuration *is* the map.
///
/// ```
/// # use terrace_config::Terrace;
/// # use terrace_config::schema::{Describe, Leaf, Refine, Refinement, Sink};
/// # struct Config;
/// # impl Describe for Config {
/// #     fn describe(sink: &mut Sink) {
/// #         sink.leaf(Leaf { name: "documents", docs: "", ty: Some("BTreeMap<String, String>"),
/// #             values: None, bounds: None, aliases: &[], note: None, required: false,
/// #             secret: false });
/// #     }
/// # }
/// /// What a legal-pages library would implement: paths relative to wherever it is mounted.
/// struct Legal;
///
/// impl Refine for Legal {
///     fn refinements(&self) -> Vec<(String, Refinement)> {
///         vec![
///             ("documents".to_owned(), Refinement::required_entries(["imprint", "privacy"])),
///             ("documents".to_owned(), Refinement::entry_names("^[a-z0-9][a-z0-9_-]{0,63}$")),
///         ]
///     }
/// }
///
/// let schema = Terrace::new("PORTFOLIO_")
///     .schema_at::<Config>("legal")
///     .refine_with("legal", &Legal)?;
///
/// let constraint = schema.keys[0].constraint.as_ref().unwrap();
/// assert_eq!(constraint["required"], serde_json::json!(["imprint", "privacy"]));
/// assert_eq!(constraint["propertyNames"]["pattern"], "^[a-z0-9][a-z0-9_-]{0,63}$");
/// # Ok::<(), terrace_config::Error>(())
/// ```
pub trait Refine {
    /// Every refinement this source publishes, as `(path relative to the mount, refinement)`.
    fn refinements(&self) -> Vec<(String, Refinement)>;
}

impl Schema {
    /// Tighten the constraint of the key at `path` beyond what its type states.
    ///
    /// Validated before anything is published, because a refinement that silently did nothing is
    /// a contract claiming a check it does not carry:
    ///
    /// - `path` must be a key's canonical path. An alias, a table, or a typo is an error naming it.
    /// - Both variants need a **map-typed** key: a constraint of `{"type": "object"}` with no
    ///   declared `properties`, whose element — when the type described one — sits under
    ///   `additionalProperties`. A map closed to every entry cannot hold one.
    /// - Every [required entry](Refinement::RequiredEntries) must be spellable wherever the key
    ///   itself is: its environment spelling nested one level down, and its secrets-directory file
    ///   name likewise. A name the environment would fold, split, or read as a `_FILE` indirection
    ///   is refused with the reason, since publishing it would name an entry some layer the key
    ///   reaches cannot supply.
    /// - An [entry-name pattern](Refinement::EntryNames) must lie inside the portable subset, and a
    ///   key carries at most one: a second, different pattern is refused rather than guessed at.
    /// - **The two must agree**, in whichever order they are applied. A required entry the pattern
    ///   rejects makes the key unsatisfiable — no map both holds the entry and names only what the
    ///   pattern admits — so the second refinement to arrive is refused, naming both.
    ///
    /// What it does:
    ///
    /// - Required entries are merged into the constraint's `required`, sorted and de-duplicated,
    ///   and unioned with any already there. The pattern is written to `propertyNames`. Refining
    ///   twice with the same refinement changes nothing.
    /// - **A default the refined constraint rejects is not a default.** If the key's
    ///   [`default_value`](Key::default_value) satisfied the constraint its type stated and fails
    ///   the refined one, the key becomes [`required`](Key::required) and loses both default
    ///   fields — the existing rule that a required key has no default, applied to a default the
    ///   image itself would refuse at boot. A default that already failed the type's own constraint
    ///   is left alone, for [`ContractBuilder::build`](super::ContractBuilder::build) to refuse:
    ///   turning it into a required key here would hide a defect in the type.
    ///   [`Self::with_defaults_from`] applies the same rule, so refining before or after observing
    ///   the defaults produces the same schema.
    /// - A key with **no** default keeps its `required` flag: whether absence is acceptable is what
    ///   the type stated, and a refinement of the value's shape does not reach it.
    /// - [`Self::schema_version`] rises to the version the refinement's keyword needs, and never
    ///   falls. See [`SCHEMA_VERSION`](super::SCHEMA_VERSION).
    ///
    /// Refine after [`Self::merge`], not before: a refined key and an unrefined one are two
    /// descriptions of one path, which is what `merge` refuses.
    ///
    /// # Errors
    /// [`Error::Invalid`] for an unknown path, a key that is not a map, an entry name that cannot be
    /// spelled, a pattern outside the portable subset, or two refinements that contradict each
    /// other — each naming the path and saying why.
    pub fn refine(mut self, path: &str, refinement: Refinement) -> Result<Self, Error> {
        let Some(index) = self.keys.iter().position(|key| key.path == path) else {
            return Err(Error::Invalid(unknown_path(&self, path)));
        };
        let dialect = self.spelling_rules();
        let version = refinement.schema_version();
        let key = &mut self.keys[index];
        let stated = key.stated.0.clone().or_else(|| key.constraint.clone());
        let changed = match refinement {
            Refinement::RequiredEntries(entries) => require_entries(key, &dialect, &entries)?,
            Refinement::EntryNames(pattern) => name_entries(key, &pattern)?,
        };
        if changed {
            key.stated = Stated(stated);
            forget_rejected_default(key);
            self.schema_version = self.schema_version.max(version);
        }
        Ok(self)
    }

    /// Apply every refinement `source` publishes, its paths read relative to `at`.
    ///
    /// `at` is where the source's configuration is mounted in this schema — `legal` for a
    /// legal-pages library read under `[legal]`. An empty `at` is the root.
    ///
    /// # Errors
    /// As [`Self::refine`], for the first refinement that fails. The message carries the full
    /// path, so a library's relative path and the host's mount point are both visible in it.
    pub fn refine_with<R: Refine + ?Sized>(mut self, at: &str, source: &R) -> Result<Self, Error> {
        for (relative, refinement) in source.refinements() {
            let path = match (at.is_empty(), relative.is_empty()) {
                (true, _) => relative,
                (false, true) => at.to_owned(),
                (false, false) => format!("{at}.{relative}"),
            };
            self = self.refine(&path, refinement)?;
        }
        Ok(self)
    }

    /// The dialect this schema was spelled under, rebuilt from what it published.
    ///
    /// Everything the spelling functions read: the prefix, the separator and the indirection
    /// suffix. Reserved names are not published and not needed — they decide whether a *file* may
    /// supply a key, not how a name is spelled.
    fn spelling_rules(&self) -> Dialect {
        Dialect::new(self.dialect.prefix.clone())
            .nesting_separator(self.dialect.nesting_separator.clone())
            .file_suffix(self.dialect.indirection_suffix.clone())
    }
}

/// The constraint a key's type stated, kept once a refinement has tightened it.
///
/// Equal to every other: it is bookkeeping behind one rule, not part of what a key is, and two keys
/// publishing the same thing are the same key whichever order their refinements and defaults
/// arrived in.
#[derive(Debug, Clone, Default)]
pub(super) struct Stated(Option<Json>);

impl PartialEq for Stated {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// Why `path` is not something a refinement can name, with what it is instead when that is known.
fn unknown_path(schema: &Schema, path: &str) -> String {
    let mut message = format!(
        "`{path}` is not a key in this schema, so a refinement of it would be published nowhere \
         while reporting success."
    );
    if let Some(key) = schema
        .keys
        .iter()
        .find(|key| key.aliases.iter().any(|alias| alias == path))
    {
        let _ = write!(
            message,
            " It is an alias of `{}`; refine the canonical path.",
            key.path
        );
    } else if schema
        .keys
        .iter()
        .any(|key| key.path.starts_with(&format!("{path}.")))
    {
        message.push_str(" It is a table; name the key inside it.");
    }
    message
}

/// [`Refinement::RequiredEntries`], applied to one key. Whether the constraint changed.
fn require_entries(
    key: &mut Key,
    dialect: &Dialect,
    entries: &BTreeSet<String>,
) -> Result<bool, Error> {
    let path = key.path.clone();
    map_constraint(key, "a required entry")?;
    for entry in entries {
        spellable(key, dialect, entry)?;
    }
    let constraint = map_constraint(key, "a required entry")?;
    if let Some(names) = constraint.get("propertyNames") {
        for entry in entries {
            if let Some(why) = refused_name(names, entry) {
                return Err(Error::Invalid(format!(
                    "`{entry}` cannot be a required entry of `{path}`: its entry names are held to \
                     {names}, and the name {why}. No map could both hold it and satisfy that."
                )));
            }
        }
    }

    let mut required: BTreeSet<String> = match constraint.get("required") {
        None => BTreeSet::new(),
        Some(Json::Array(names)) if names.iter().all(Json::is_string) => names
            .iter()
            .filter_map(Json::as_str)
            .map(ToOwned::to_owned)
            .collect(),
        Some(other) => {
            return Err(Error::Invalid(format!(
                "`{path}` already carries `required: {other}`, which is not a list of entry names, \
                 so there is nothing sound to union a refinement with."
            )));
        }
    };
    let before = required.len();
    required.extend(entries.iter().cloned());
    if required.len() == before {
        return Ok(false);
    }
    constraint.insert(
        "required".to_owned(),
        Json::Array(required.into_iter().map(Json::String).collect()),
    );
    Ok(true)
}

/// [`Refinement::EntryNames`], applied to one key. Whether the constraint changed.
fn name_entries(key: &mut Key, source: &str) -> Result<bool, Error> {
    let path = key.path.clone();
    let constraint = map_constraint(key, "an entry-name pattern")?;
    if let Err(why) = pattern::check(source) {
        return Err(Error::Invalid(format!(
            "`{source}` cannot be the entry-name pattern of `{path}`: {why}. A published pattern \
             must mean the same thing to every engine that reads the contract; see *Portable \
             patterns* in spec/v1/FORMAT.md."
        )));
    }

    let names = serde_json::json!({ "pattern": source });
    match constraint.get("propertyNames") {
        Some(held) if *held == names => return Ok(false),
        Some(held) => {
            return Err(Error::Invalid(format!(
                "`{path}` already holds its entry names to {held}, so a second pattern `{source}` \
                 would need both to hold. Publish one pattern that says so."
            )));
        }
        None => {}
    }
    for entry in constraint
        .get("required")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
    {
        if let Some(why) = refused_name(&names, entry) {
            return Err(Error::Invalid(format!(
                "`{source}` cannot be the entry-name pattern of `{path}`: `{path}` requires the \
                 entry `{entry}`, and the name {why}. No map could both hold it and satisfy the \
                 pattern."
            )));
        }
    }
    constraint.insert("propertyNames".to_owned(), names);
    Ok(true)
}

/// Why `name` fails the entry-name schema `names`, when it certainly does.
fn refused_name(names: &Json, name: &str) -> Option<String> {
    match check::verdict(names, &Json::String(name.to_owned())) {
        Verdict::Fails(why) => Some(why),
        Verdict::Holds | Verdict::Undecided => None,
    }
}

/// The key's constraint, when it describes a map `what` can be stated about.
fn map_constraint<'a>(key: &'a mut Key, what: &str) -> Result<&'a mut Map<String, Json>, Error> {
    let path = key.path.clone();
    let Some(Json::Object(constraint)) = key.constraint.as_mut() else {
        return Err(Error::Invalid(format!(
            "`{path}` publishes no constraint, so nothing says it is a map and {what} has nowhere \
             to be stated. Refine a key whose type is a map."
        )));
    };
    let is_object = constraint.get("type").and_then(Json::as_str) == Some("object");
    let declares_fields = constraint.contains_key("properties");
    // Absent, `true` or an element schema: every one of those admits an entry. `false` admits
    // none, and requiring one would publish a key no value satisfies.
    let open = constraint
        .get("additionalProperties")
        .is_none_or(|element| element.is_object() || element == &Json::Bool(true));
    if !is_object || declares_fields || !open {
        let shape = Json::Object(constraint.clone());
        return Err(Error::Invalid(format!(
            "`{path}` is not a map, so it has no entries for {what} to describe: its constraint is \
             {shape}. That needs an object whose entries are open, with any element shape under \
             `additionalProperties`."
        )));
    }
    Ok(constraint)
}

/// Refuse an entry name some layer reaching the key could not spell.
fn spellable(key: &Key, dialect: &Dialect, entry: &str) -> Result<(), Error> {
    let path = &key.path;
    let refuse = |why: String| {
        Err(Error::Invalid(format!(
            "`{entry}` cannot be a required entry of `{path}`: {why}"
        )))
    };
    if entry.is_empty() {
        return refuse("an empty name is not an entry any layer can supply.".to_owned());
    }
    if entry.contains('.') {
        return refuse(format!(
            "the loader reads `.` as nesting, so `{path}.{entry}` names a table inside the map \
             rather than one entry of it."
        ));
    }

    let entry_path = format!("{path}.{entry}");
    if key.env.is_some() && !key.reserved {
        let (env, reason) = env_spelling(dialect, &entry_path);
        if env.is_none() {
            let spelled = dialect.env_spelling(&entry_path);
            return refuse(if reason == Some(Unreachable::Indirection) {
                format!(
                    "`{spelled}` is read as the `{}` indirection for another name, so no \
                     variable supplies this entry.",
                    dialect.indirection_suffix()
                )
            } else {
                let arrives = env_layer_key(dialect, &spelled)
                    .map_or_else(|| "nothing".to_owned(), |key| format!("`{key}`"));
                format!(
                    "`{spelled}` is the variable that would supply it, and the environment layer \
                     reads that as {arrives}. Name the entry in lower case, without the \
                     `{}` separator.",
                    dialect.separator()
                )
            });
        }
    }
    if key.secrets_file.is_some() && secrets_file_name(dialect, &entry_path).is_none() {
        return refuse(format!(
            "no file in the secrets directory can be named for `{entry_path}`, which `{path}` \
             itself can be supplied from."
        ));
    }
    Ok(())
}

/// Drop a default the refinement just made invalid, making the key required.
fn forget_rejected_default(key: &mut Key) {
    if key
        .default_value
        .as_ref()
        .is_some_and(|value| rejected_by_refinement(key, value))
    {
        key.required = true;
        key.default = None;
        key.default_value = None;
    }
}

/// The map-typed constraint a key publishes, when it is one.
fn map_of(key: &Key) -> Option<&Map<String, Json>> {
    let Some(Json::Object(constraint)) = &key.constraint else {
        return None;
    };
    (constraint.get("type").and_then(Json::as_str) == Some("object")
        && !constraint.contains_key("properties"))
    .then_some(constraint)
}

/// The entries a map-typed key's constraint requires, in the order it publishes them.
///
/// The single source every rendering reads: there is no parallel field on [`Key`] that could come
/// to disagree with the constraint a validator actually applies.
pub(super) fn required_entries(key: &Key) -> Vec<&str> {
    map_of(key)
        .and_then(|constraint| constraint.get("required"))
        .and_then(Json::as_array)
        .map(|names| names.iter().filter_map(Json::as_str).collect())
        .unwrap_or_default()
}

/// The pattern a map-typed key's constraint holds its entry names to, when it has one.
///
/// Read from `propertyNames`, as [`required_entries`] reads `required`, for the same reason.
pub(super) fn entry_name_pattern(key: &Key) -> Option<&str> {
    map_of(key)?.get("propertyNames")?.get("pattern")?.as_str()
}

/// Whether an observed default is one the key's *refinements* reject.
///
/// Only a failure the refinements introduced counts: the default satisfies the constraint the type
/// stated and fails the refined one. A default failing for some *other* reason — a wrong type, an
/// element out of range — is a defect in what the type published, which
/// [`ContractBuilder::build`](super::ContractBuilder::build) refuses; turning it into a required
/// key here would hide it.
pub(super) fn rejected_by_refinement(key: &Key, value: &figment::value::Value) -> bool {
    let (Some(refined), Ok(value)) = (&key.constraint, serde_json::to_value(value)) else {
        return false;
    };
    if !matches!(check::verdict(refined, &value), Verdict::Fails(_)) {
        return false;
    }
    // A key never refined has nothing but what its type stated, and that failure is the type's.
    key.stated
        .0
        .as_ref()
        .is_some_and(|stated| !matches!(check::verdict(stated, &value), Verdict::Fails(_)))
}
