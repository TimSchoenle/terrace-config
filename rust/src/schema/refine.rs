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
    Key, LATEST_SCHEMA_VERSION, MAX_DEPTH, Schema, Unreachable, env_layer_key, env_spelling,
    pattern, secrets_file_name,
};

/// The path segment that addresses every element of a map or a sequence.
///
/// An element has no name of its own — a map's entries are the operator's to choose, and a
/// sequence's are counted — so a refinement reaching inside one names the element schema all of
/// them share, not any one of them.
pub const ELEMENT: &str = "*";

/// The segment standing in for an element when an entry name inside one is spelled for the
/// environment: any name the operator could choose, and one every layer can spell.
const SPELLED_ELEMENT: &str = "entry";

/// One tightening of a key's constraint, beyond what its type states.
///
/// `#[non_exhaustive]` because the set grows, and every variant has to meet the bar the first one
/// does: it tightens, it is checkable by [`Schema::refine`] before it is published, and it has a
/// meaning every rendering can show.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refinement {
    /// A map must contain these entries, whatever else it holds.
    ///
    /// Published as JSON Schema's `required` at the map's position in the key's
    /// [`constraint`](super::Key::constraint), unioned with any entries already required. The map
    /// stays open: an entry not named here is still accepted, exactly as the type accepts it.
    ///
    /// An empty set is **accepted and changes nothing**, once the path and the map's shape have
    /// been checked. A library computing its required entries at runtime can legitimately compute
    /// none, and refusing that would make every caller special-case it — while skipping the checks
    /// for an empty set would let a mistyped path through exactly when the set happens to be empty.
    RequiredEntries(BTreeSet<String>),
    /// Every entry name of a map must match this pattern.
    ///
    /// Published as JSON Schema's `propertyNames: {"pattern": …}` at the map's position. The
    /// pattern is matched as JSON Schema matches one — unanchored, so a pattern meaning the whole
    /// name says so with `^` and `$` — and it must lie inside the *portable subset*: the constructs
    /// every engine a contract is read by agrees on. [`Schema::refine`] refuses anything outside
    /// it, naming the construct and the portable spelling. The subset is set out in
    /// `spec/v1/FORMAT.md`, under *Portable patterns*; in short, no `.`, no `\d`, `\w`, `\s` or
    /// `\b`, no lookaround and no lazy repetition — a class naming exactly the characters meant
    /// says the same thing in every engine.
    ///
    /// **The environment folds names to lower case.** A pattern admitting an upper-case name
    /// describes an entry no environment variable can supply, since `PORTFOLIO_LEGAL__DOCUMENTS__TERMS`
    /// arrives as `terms` whatever case it was written in. That is weaker than the loader, not wrong
    /// — the entry is still supplied from a file — so it is published as written; a pattern meant to
    /// describe what every layer can supply names lower-case characters only.
    EntryNames(String),
    /// A string must match this pattern.
    ///
    /// Published as JSON Schema's `pattern` at the string's position — a string-typed key, or a
    /// string inside one's element schema. The same portable subset as [`Self::EntryNames`], matched
    /// the same way. [`Self::non_blank`] is the one pattern common enough, and easy enough to get
    /// wrong, to ship ready-made.
    Pattern(String),
}

impl Refinement {
    /// Text holding at least one character that is not white space — the negation of Rust's
    /// `str::trim().is_empty()`, exactly.
    ///
    /// The class is Unicode's `White_Space` property, which is what [`char::is_whitespace`] tests.
    /// **`\S` is not a substitute**, and the subset refuses it: ECMA-262's `\s` holds U+FEFF, which
    /// is not `White_Space`, so a pattern of `\S` rejects a value consisting of U+FEFF that
    /// `trim()` leaves non-empty — a contract stricter than the check it describes.
    pub const NON_BLANK: &str = "[^\\t\\n\\u000B\\f\\r \\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000]";

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

    /// [`Self::Pattern`] from a pattern, checked when it is applied.
    #[must_use]
    pub fn pattern(pattern: impl Into<String>) -> Self {
        Self::Pattern(pattern.into())
    }

    /// [`Self::Pattern`] of [`Self::NON_BLANK`]: the string is not empty and not only white space.
    #[must_use]
    pub fn non_blank() -> Self {
        Self::Pattern(Self::NON_BLANK.to_owned())
    }

    /// The version of the schema half a document carrying this refinement is published at.
    const fn schema_version(&self) -> u32 {
        match self {
            Self::RequiredEntries(_) | Self::Pattern(_) => super::SCHEMA_VERSION,
            Self::EntryNames(_) => LATEST_SCHEMA_VERSION,
        }
    }

    /// What the refinement states, as a refusal names it.
    const fn what(&self) -> &'static str {
        match self {
            Self::RequiredEntries(_) => "a required entry",
            Self::EntryNames(_) => "an entry-name pattern",
            Self::Pattern(_) => "a pattern",
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
///             // Every document's text, wherever the map is mounted.
///             ("documents.*".to_owned(), Refinement::non_blank()),
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
/// assert_eq!(constraint["additionalProperties"]["pattern"], Refinement::NON_BLANK);
/// # Ok::<(), terrace_config::Error>(())
/// ```
pub trait Refine {
    /// Every refinement this source publishes, as `(path relative to the mount, refinement)`.
    fn refinements(&self) -> Vec<(String, Refinement)>;
}

impl Schema {
    /// Tighten the constraint at `path` beyond what its type states.
    ///
    /// `path` is a key's canonical path, optionally continued into the key's constraint: a
    /// [`ELEMENT`] segment (`*`) steps into the element every entry of a map or every item of a
    /// sequence shares, and a field name steps into a struct's field. So `legal.documents.*.body`
    /// is the `body` field of every document in the `legal.documents` map, and
    /// `legal.documents.*.body.*` is every text in that map of texts. Only what the type described
    /// can be addressed: a map whose element the walk did not read, a field that is not declared,
    /// or a single entry of a map — whose name the operator chooses, so it is no position of the
    /// schema — is refused with the reason.
    ///
    /// Validated before anything is published, because a refinement that silently did nothing is
    /// a contract claiming a check it does not carry:
    ///
    /// - The key must exist under its canonical path. An alias, a table, or a typo is an error
    ///   naming it.
    /// - [`Refinement::RequiredEntries`] and [`Refinement::EntryNames`] need a **map** at the
    ///   position: `{"type": "object"}` with no declared `properties`, whose element — when the type
    ///   described one — sits under `additionalProperties`. A map closed to every entry cannot hold
    ///   one. [`Refinement::Pattern`] needs a **string**.
    /// - Every [required entry](Refinement::RequiredEntries) must be spellable wherever the key
    ///   itself is: its environment spelling nested down to the entry, and its secrets-directory
    ///   file name likewise. A name the environment would fold, split, or read as a `_FILE`
    ///   indirection is refused with the reason, since publishing it would name an entry some layer
    ///   the key reaches cannot supply.
    /// - A pattern must lie inside the portable subset, and a position carries at most one of each
    ///   kind: a second, different pattern is refused rather than guessed at.
    /// - **Refinements must agree**, in whichever order they are applied. A required entry the
    ///   entry-name pattern rejects makes the map unsatisfiable, and so does a pattern no value of a
    ///   choice matches; the second refinement to arrive is refused, naming both.
    ///
    /// What it does:
    ///
    /// - Required entries are merged into the position's `required`, sorted and de-duplicated, and
    ///   unioned with any already there. An entry-name pattern is written to `propertyNames`, a
    ///   pattern to `pattern`. Refining twice with the same refinement changes nothing.
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
    /// [`Error::Invalid`] for an unknown path, a position the type did not describe, a position of
    /// the wrong shape, an entry name that cannot be spelled, a pattern outside the portable
    /// subset, or two refinements that contradict each other — each naming the path and saying why.
    pub fn refine(mut self, path: &str, refinement: Refinement) -> Result<Self, Error> {
        let (index, inside) = locate(&self, path)?;
        let dialect = self.spelling_rules();
        let version = refinement.schema_version();
        let what = refinement.what();
        let key = &mut self.keys[index];
        let reach = Reach::of(key, &inside);
        let stated = key.stated.0.clone().or_else(|| key.constraint.clone());

        let Some(Json::Object(root)) = key.constraint.as_mut() else {
            return Err(Error::Invalid(format!(
                "`{}` publishes no constraint, so nothing says what it holds and {} has nowhere to \
                 be stated. Refine a key whose type this crate reads.",
                key.path, what
            )));
        };
        let target = descend(root, &key.path, &inside)?;
        let changed = match refinement {
            Refinement::RequiredEntries(entries) => {
                require_entries(target, &reach, &dialect, &entries)?
            }
            Refinement::EntryNames(source) => name_entries(target, &reach.at, &source)?,
            Refinement::Pattern(source) => match_pattern(target, &reach.at, &source)?,
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

/// Where a refinement lands, and which layers reach it.
struct Reach {
    /// The position as written: the key's path, then any segments inside it.
    at: String,
    /// The same position with every [`ELEMENT`] segment replaced by a name every layer can spell,
    /// for spelling an entry below it.
    spelled: String,
    /// Whether an environment variable can supply the key, and so an entry inside it.
    env: bool,
    /// Whether a secrets-directory file can.
    secrets_file: bool,
}

impl Reach {
    fn of(key: &Key, inside: &[String]) -> Self {
        let mut at = key.path.clone();
        let mut spelled = key.path.clone();
        for segment in inside {
            at.push('.');
            at.push_str(segment);
            spelled.push('.');
            spelled.push_str(if segment == ELEMENT {
                SPELLED_ELEMENT
            } else {
                segment
            });
        }
        Self {
            at,
            spelled,
            env: key.env.is_some() && !key.reserved,
            secrets_file: key.secrets_file.is_some(),
        }
    }
}

/// The key `path` names, and the segments of `path` that continue inside its constraint.
///
/// A key's own path wins; otherwise the longest key path `path` continues. Keys never nest inside
/// one another — a key is a leaf of the document — so at most one can be a proper prefix.
fn locate(schema: &Schema, path: &str) -> Result<(usize, Vec<String>), Error> {
    if let Some(index) = schema.keys.iter().position(|key| key.path == path) {
        return Ok((index, Vec::new()));
    }
    let Some((index, key)) = schema
        .keys
        .iter()
        .enumerate()
        .filter(|(_, key)| path.starts_with(&format!("{}.", key.path)))
        .max_by_key(|(_, key)| key.path.len())
    else {
        return Err(Error::Invalid(unknown_path(schema, path)));
    };
    let inside: Vec<String> = path[key.path.len() + 1..]
        .split('.')
        .map(ToOwned::to_owned)
        .collect();
    if inside.iter().any(String::is_empty) {
        return Err(Error::Invalid(format!(
            "`{path}` has an empty segment, so it names no position inside `{}`.",
            key.path
        )));
    }
    Ok((index, inside))
}

/// Why `path` is not something a refinement can name, with what it is instead when that is known.
fn unknown_path(schema: &Schema, path: &str) -> String {
    let mut message = format!(
        "`{path}` is not a key in this schema, so a refinement of it would be published nowhere \
         while reporting success."
    );
    if let Some(key) = schema.keys.iter().find(|key| {
        key.aliases
            .iter()
            .any(|alias| alias == path || path.starts_with(&format!("{alias}.")))
    }) {
        let _ = write!(
            message,
            " It is an alias of `{}`, or inside one; refine the canonical path.",
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

/// The position `inside` names within a key's constraint, or why there is none.
fn descend<'a>(
    root: &'a mut Map<String, Json>,
    key_path: &str,
    inside: &[String],
) -> Result<&'a mut Map<String, Json>, Error> {
    let mut at = key_path.to_owned();
    let mut schema = root;
    for segment in inside {
        schema = if segment == ELEMENT {
            element(schema, &at)?
        } else {
            field(schema, &at, segment)?
        };
        at.push('.');
        at.push_str(segment);
    }
    Ok(schema)
}

/// The element schema of the map or sequence at `at`.
fn element<'a>(
    schema: &'a mut Map<String, Json>,
    at: &str,
) -> Result<&'a mut Map<String, Json>, Error> {
    let keyword = if is_map(schema) {
        "additionalProperties"
    } else if schema.get("type").and_then(Json::as_str) == Some("array") {
        "items"
    } else {
        return Err(Error::Invalid(format!(
            "`{at}` is neither a map nor a sequence, so it has no element for `{ELEMENT}` to \
             address: its constraint is {}.",
            Json::Object(schema.clone())
        )));
    };
    match schema.get_mut(keyword) {
        Some(Json::Object(element)) => Ok(element),
        _ => Err(Error::Invalid(format!(
            "`{at}` publishes no element schema, so `{at}.{ELEMENT}` names nothing a refinement \
             could be stated on. Describe the element — `#[config(element)]` on the field — or \
             refine `{at}` itself."
        ))),
    }
}

/// The schema of the declared field `name` of the struct at `at`.
fn field<'a>(
    schema: &'a mut Map<String, Json>,
    at: &str,
    name: &str,
) -> Result<&'a mut Map<String, Json>, Error> {
    let declared: Vec<String> = schema
        .get("properties")
        .and_then(Json::as_object)
        .map(|fields| fields.keys().cloned().collect())
        .unwrap_or_default();
    if declared.iter().any(|field| field == name) {
        return match schema
            .get_mut("properties")
            .and_then(Json::as_object_mut)
            .and_then(|fields| fields.get_mut(name))
        {
            Some(Json::Object(field)) => Ok(field),
            _ => Err(Error::Invalid(format!(
                "`{at}.{name}` publishes no schema a refinement could be stated on."
            ))),
        };
    }
    if schema.get("type").and_then(Json::as_str) != Some("object") {
        return Err(Error::Invalid(format!(
            "`{at}` is not a struct, so it has no field `{name}`: its constraint is {}.",
            Json::Object(schema.clone())
        )));
    }
    if is_map(schema) {
        return Err(Error::Invalid(format!(
            "`{at}` is a map, and `{name}` would be one entry of it. An entry's name is the \
             operator's to choose, so it is no position of the schema; address every entry with \
             `{at}.{ELEMENT}`."
        )));
    }
    let fields = if declared.is_empty() {
        " It declares no fields.".to_owned()
    } else {
        format!(
            " Its fields are {}.",
            declared
                .iter()
                .map(|field| format!("`{field}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    Err(Error::Invalid(format!(
        "`{name}` is not a field of `{at}`, so `{at}.{name}` names nothing.{fields}"
    )))
}

/// Whether a schema describes a map: an object whose entries are not declared fields.
fn is_map(schema: &Map<String, Json>) -> bool {
    schema.get("type").and_then(Json::as_str) == Some("object")
        && !schema.contains_key("properties")
}

/// [`Refinement::RequiredEntries`], applied at one position. Whether the constraint changed.
fn require_entries(
    target: &mut Map<String, Json>,
    reach: &Reach,
    dialect: &Dialect,
    entries: &BTreeSet<String>,
) -> Result<bool, Error> {
    let at = &reach.at;
    map_constraint(target, at, "a required entry")?;
    for entry in entries {
        spellable(reach, dialect, entry)?;
    }
    if let Some(names) = target.get("propertyNames") {
        for entry in entries {
            if let Some(why) = refused(names, entry) {
                return Err(Error::Invalid(format!(
                    "`{entry}` cannot be a required entry of `{at}`: its entry names are held to \
                     {names}, and the name {why}. No map could both hold it and satisfy that."
                )));
            }
        }
    }

    let mut required: BTreeSet<String> = match target.get("required") {
        None => BTreeSet::new(),
        Some(Json::Array(names)) if names.iter().all(Json::is_string) => names
            .iter()
            .filter_map(Json::as_str)
            .map(ToOwned::to_owned)
            .collect(),
        Some(other) => {
            return Err(Error::Invalid(format!(
                "`{at}` already carries `required: {other}`, which is not a list of entry names, \
                 so there is nothing sound to union a refinement with."
            )));
        }
    };
    let before = required.len();
    required.extend(entries.iter().cloned());
    if required.len() == before {
        return Ok(false);
    }
    target.insert(
        "required".to_owned(),
        Json::Array(required.into_iter().map(Json::String).collect()),
    );
    Ok(true)
}

/// [`Refinement::EntryNames`], applied at one position. Whether the constraint changed.
fn name_entries(target: &mut Map<String, Json>, at: &str, source: &str) -> Result<bool, Error> {
    map_constraint(target, at, "an entry-name pattern")?;
    portable(source, "entry-name pattern", at)?;

    let names = serde_json::json!({ "pattern": source });
    match target.get("propertyNames") {
        Some(held) if *held == names => return Ok(false),
        Some(held) => {
            return Err(Error::Invalid(format!(
                "`{at}` already holds its entry names to {held}, so a second pattern `{source}` \
                 would need both to hold. Publish one pattern that says so."
            )));
        }
        None => {}
    }
    for entry in target
        .get("required")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
    {
        if let Some(why) = refused(&names, entry) {
            return Err(Error::Invalid(format!(
                "`{source}` cannot be the entry-name pattern of `{at}`: `{at}` requires the entry \
                 `{entry}`, and the name {why}. No map could both hold it and satisfy the pattern."
            )));
        }
    }
    target.insert("propertyNames".to_owned(), names);
    Ok(true)
}

/// [`Refinement::Pattern`], applied at one position. Whether the constraint changed.
fn match_pattern(target: &mut Map<String, Json>, at: &str, source: &str) -> Result<bool, Error> {
    let is_string = match target.get("type") {
        Some(Json::String(name)) => name == "string",
        Some(Json::Array(names)) => names.iter().any(|name| name == "string"),
        _ => false,
    };
    if !is_string {
        return Err(Error::Invalid(format!(
            "`{at}` is not a string, so it has no text for a pattern to match: its constraint is \
             {}.",
            Json::Object(target.clone())
        )));
    }
    portable(source, "pattern", at)?;

    match target.get("pattern") {
        Some(Json::String(held)) if held == source => return Ok(false),
        Some(held) => {
            return Err(Error::Invalid(format!(
                "`{at}` is already matched against {held}, so a second pattern `{source}` would \
                 need both to hold. Publish one pattern that says so."
            )));
        }
        None => {}
    }
    let matching = serde_json::json!({ "pattern": source });
    if let Some(Json::Array(choices)) = target.get("enum")
        && !choices.is_empty()
        && choices.iter().all(|choice| {
            choice
                .as_str()
                .is_some_and(|text| refused(&matching, text).is_some())
        })
    {
        return Err(Error::Invalid(format!(
            "`{source}` cannot be the pattern of `{at}`: `{at}` is one of {}, and the pattern \
             matches none of them. No value could satisfy both.",
            Json::Array(choices.clone())
        )));
    }
    target.insert("pattern".to_owned(), Json::String(source.to_owned()));
    Ok(true)
}

/// Refuse a pattern outside the portable subset.
fn portable(source: &str, what: &str, at: &str) -> Result<(), Error> {
    pattern::check(source).map_err(|why| {
        Error::Invalid(format!(
            "`{source}` cannot be the {what} of `{at}`: {why}. A published pattern must mean the \
             same thing to every engine that reads the contract; see *Portable patterns* in \
             spec/v1/FORMAT.md."
        ))
    })
}

/// Why `text` fails `schema`, when it certainly does.
fn refused(schema: &Json, text: &str) -> Option<String> {
    match check::verdict(schema, &Json::String(text.to_owned())) {
        Verdict::Fails(why) => Some(why),
        Verdict::Holds | Verdict::Undecided => None,
    }
}

/// Refuse a schema at `at` that is not a map `what` can be stated about.
fn map_constraint(schema: &Map<String, Json>, at: &str, what: &str) -> Result<(), Error> {
    // Absent, `true` or an element schema: every one of those admits an entry. `false` admits
    // none, and requiring one would publish a map no value satisfies.
    let open = schema
        .get("additionalProperties")
        .is_none_or(|element| element.is_object() || element == &Json::Bool(true));
    if is_map(schema) && open {
        return Ok(());
    }
    Err(Error::Invalid(format!(
        "`{at}` is not a map, so it has no entries for {what} to describe: its constraint is {}. \
         That needs an object whose entries are open, with any element shape under \
         `additionalProperties`.",
        Json::Object(schema.clone())
    )))
}

/// Refuse an entry name some layer reaching the map could not spell.
fn spellable(reach: &Reach, dialect: &Dialect, entry: &str) -> Result<(), Error> {
    let at = &reach.at;
    let refuse = |why: String| {
        Err(Error::Invalid(format!(
            "`{entry}` cannot be a required entry of `{at}`: {why}"
        )))
    };
    if entry.is_empty() {
        return refuse("an empty name is not an entry any layer can supply.".to_owned());
    }
    if entry.contains('.') {
        return refuse(format!(
            "the loader reads `.` as nesting, so `{at}.{entry}` names a table inside the map \
             rather than one entry of it."
        ));
    }

    let entry_path = format!("{}.{entry}", reach.spelled);
    if reach.env {
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
    if reach.secrets_file && secrets_file_name(dialect, &entry_path).is_none() {
        return refuse(format!(
            "no file in the secrets directory can be named for `{entry_path}`, which the key \
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

/// One tightening a constraint publishes, as every rendering names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Tightening<'a> {
    /// A map's `required` entries.
    Entries(Vec<&'a str>),
    /// A map's `propertyNames` pattern.
    Names(&'a str),
    /// A string's `pattern`.
    Matches(&'a str),
}

/// Every tightening a key's constraint publishes, each with its position relative to the key —
/// empty for the key itself, `*.body` for the `body` field of every element.
///
/// The single source every rendering reads: there is no parallel field on [`Key`] that could come
/// to disagree with the constraint a validator actually applies. In the order a reader meets them:
/// the key's own first — entries, entry names, pattern — then its element, then its fields in the
/// order the constraint lists them.
///
/// Only what a refinement can write is read. A struct's `required` names its fields, not a map's
/// entries, and is not one; a `propertyNames` other than a lone pattern is not a refinement this
/// crate writes.
pub(super) fn tightenings(key: &Key) -> Vec<(String, Tightening<'_>)> {
    let mut found = Vec::new();
    if let Some(constraint) = &key.constraint {
        walk(constraint, "", 0, &mut found);
    }
    found
}

fn walk<'a>(schema: &'a Json, at: &str, depth: usize, found: &mut Vec<(String, Tightening<'a>)>) {
    let Some(schema) = schema.as_object() else {
        return;
    };
    if depth > MAX_DEPTH {
        return;
    }
    if is_map(schema) {
        let entries: Vec<&str> = schema
            .get("required")
            .and_then(Json::as_array)
            .map(|names| names.iter().filter_map(Json::as_str).collect())
            .unwrap_or_default();
        if !entries.is_empty() {
            found.push((at.to_owned(), Tightening::Entries(entries)));
        }
        if let Some(pattern) = schema
            .get("propertyNames")
            .and_then(Json::as_object)
            .filter(|names| names.len() == 1)
            .and_then(|names| names.get("pattern"))
            .and_then(Json::as_str)
        {
            found.push((at.to_owned(), Tightening::Names(pattern)));
        }
    }
    if let Some(pattern) = schema.get("pattern").and_then(Json::as_str) {
        found.push((at.to_owned(), Tightening::Matches(pattern)));
    }

    let below = |segment: &str| {
        if at.is_empty() {
            segment.to_owned()
        } else {
            format!("{at}.{segment}")
        }
    };
    for keyword in ["additionalProperties", "items"] {
        if let Some(element) = schema.get(keyword).filter(|element| element.is_object()) {
            walk(element, &below(ELEMENT), depth + 1, found);
        }
    }
    if let Some(fields) = schema.get("properties").and_then(Json::as_object) {
        for (name, field) in fields {
            walk(field, &below(name), depth + 1, found);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::Refinement;
    use crate::schema::pattern;

    #[test]
    fn non_blank_is_exactly_what_trim_leaves_non_empty() {
        let blank = pattern::matcher(Refinement::NON_BLANK).expect("portable");
        // Every scalar value, one at a time: the class must be `char::is_whitespace` and nothing
        // else, so a one-character string matches exactly when it is not white space.
        for code in 0..=0x10_FFFF_u32 {
            let Some(character) = char::from_u32(code) else {
                continue;
            };
            assert_eq!(
                blank.is_match(character.encode_utf8(&mut [0; 4])),
                !character.is_whitespace(),
                "U+{code:04X}"
            );
        }
        assert!(!blank.is_match(""));
        assert!(!blank.is_match(" \t\u{3000}\n"));
        assert!(blank.is_match(" \u{feff} "));
    }
}
