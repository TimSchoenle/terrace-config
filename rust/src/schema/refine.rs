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

use super::{Key, Schema, Unreachable, env_layer_key, env_spelling, secrets_file_name};

/// One tightening of a key's constraint, beyond what its type states.
///
/// `#[non_exhaustive]` because the set grows — an entry-name pattern published as `propertyNames`
/// is the obvious next one — and every variant has to meet the bar the first one does: it
/// tightens, it is checkable by [`Schema::refine`] before it is published, and it has a meaning
/// every rendering can show.
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
///         vec![("documents".to_owned(), Refinement::required_entries(["imprint", "privacy"]))]
///     }
/// }
///
/// let schema = Terrace::new("PORTFOLIO_")
///     .schema_at::<Config>("legal")
///     .refine_with("legal", &Legal)?;
///
/// let key = &schema.keys[0];
/// assert_eq!(key.constraint.as_ref().unwrap()["required"], serde_json::json!(["imprint", "privacy"]));
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
    /// - [`Refinement::RequiredEntries`] needs a **map-typed** key: a constraint of
    ///   `{"type": "object"}` with no declared `properties`, whose element — when the type described
    ///   one — sits under `additionalProperties`. A map closed to every entry cannot hold one.
    /// - Every entry name must be spellable wherever the key itself is: its environment spelling
    ///   nested one level down, and its secrets-directory file name likewise. A name the
    ///   environment would fold, split, or read as a `_FILE` indirection is refused with the
    ///   reason, since publishing it would name an entry some layer the key reaches cannot supply.
    ///
    /// What it does:
    ///
    /// - The entries are merged into the constraint's `required`, sorted and de-duplicated, and
    ///   unioned with any already there. Refining twice with the same set changes nothing.
    /// - **A default the refined constraint rejects is not a default.** If the key's
    ///   [`default_value`](Key::default_value) is a map lacking a required entry, the key becomes
    ///   [`required`](Key::required) and loses both default fields — the existing rule that a
    ///   required key has no default, applied to a default the image itself would refuse at boot.
    ///   [`Self::with_defaults_from`] applies the same rule, so refining before or after observing
    ///   the defaults produces the same schema.
    /// - A key with **no** default keeps its `required` flag: whether absence is acceptable is what
    ///   the type stated, and a refinement of the value's shape does not reach it.
    ///
    /// Refine after [`Self::merge`], not before: a refined key and an unrefined one are two
    /// descriptions of one path, which is what `merge` refuses.
    ///
    /// # Errors
    /// [`Error::Invalid`] for an unknown path, a key that is not a map, or an entry name that
    /// cannot be spelled — each naming the path and saying why.
    pub fn refine(mut self, path: &str, refinement: Refinement) -> Result<Self, Error> {
        let Some(index) = self.keys.iter().position(|key| key.path == path) else {
            return Err(Error::Invalid(unknown_path(&self, path)));
        };
        let dialect = self.spelling_rules();
        let key = &mut self.keys[index];
        match refinement {
            Refinement::RequiredEntries(entries) => require_entries(key, &dialect, &entries)?,
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

/// [`Refinement::RequiredEntries`], applied to one key.
fn require_entries(
    key: &mut Key,
    dialect: &Dialect,
    entries: &BTreeSet<String>,
) -> Result<(), Error> {
    let path = key.path.clone();
    map_constraint(key)?;
    for entry in entries {
        spellable(key, dialect, entry)?;
    }
    if entries.is_empty() {
        return Ok(());
    }

    let constraint = map_constraint(key)?;
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
    required.extend(entries.iter().cloned());
    constraint.insert(
        "required".to_owned(),
        Json::Array(required.into_iter().map(Json::String).collect()),
    );

    if key
        .default_value
        .as_ref()
        .is_some_and(|value| lacks_required_entries(key, value))
    {
        key.required = true;
        key.default = None;
        key.default_value = None;
    }
    Ok(())
}

/// The key's constraint, when it describes a map a required entry can be added to.
fn map_constraint(key: &mut Key) -> Result<&mut Map<String, Json>, Error> {
    let path = key.path.clone();
    let Some(Json::Object(constraint)) = key.constraint.as_mut() else {
        return Err(Error::Invalid(format!(
            "`{path}` publishes no constraint, so nothing says it is a map and a required entry \
             has nowhere to be stated. Refine a key whose type is a map."
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
            "`{path}` is not a map, so it has no entries to require: its constraint is {shape}. \
             A required entry needs an object whose entries are open, with any element shape \
             under `additionalProperties`."
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

/// The entries a map-typed key's constraint requires, in the order it publishes them.
///
/// The single source every rendering reads: there is no parallel field on [`Key`] that could come
/// to disagree with the constraint a validator actually applies.
pub(super) fn required_entries(key: &Key) -> Vec<&str> {
    let Some(Json::Object(constraint)) = &key.constraint else {
        return Vec::new();
    };
    if constraint.get("type").and_then(Json::as_str) != Some("object")
        || constraint.contains_key("properties")
    {
        return Vec::new();
    }
    constraint
        .get("required")
        .and_then(Json::as_array)
        .map(|names| names.iter().filter_map(Json::as_str).collect())
        .unwrap_or_default()
}

/// Whether an observed default leaves out an entry the key's constraint requires.
///
/// Only the refinement's own keyword is read. A default failing the constraint for some *other*
/// reason — a wrong type, an element out of range — is a defect in what the type published, which
/// [`ContractBuilder::build`](super::ContractBuilder::build) refuses; turning it into a required
/// key here would hide it.
pub(super) fn lacks_required_entries(key: &Key, value: &figment::value::Value) -> bool {
    let figment::value::Value::Dict(_, entries) = value else {
        return false;
    };
    required_entries(key)
        .iter()
        .any(|name| !entries.contains_key(*name))
}
