//! How a deployment spells a configuration key in its environment.
//!
//! Every name this crate reads or reports is derived from one [`Dialect`]: the prefix, the
//! nesting separator, the `_FILE` suffix, and the handful of keys that are read before the
//! layers exist. It is the whole of the parameterisation that turned a project-specific loader
//! into a crate — in the original there were four `const`s here and a hardcoded array.

use std::collections::{BTreeMap, BTreeSet};

/// The default separator between nesting levels in an environment key.
const DEFAULT_SEPARATOR: &str = "__";

/// The default suffix marking a variable that names a *file* holding a value rather than
/// holding the value itself.
const DEFAULT_FILE_SUFFIX: &str = "_FILE";

/// The environment spelling of one application's configuration keys.
///
/// ```
/// use terrace_config::Dialect;
///
/// let dialect = Dialect::new("MYAPP_");
/// assert_eq!(dialect.key_path("AUTH__JWT_SECRET"), "auth.jwt_secret");
/// assert_eq!(dialect.env_spelling("auth.jwt_secret"), "MYAPP_AUTH__JWT_SECRET");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialect {
    /// The prefix every configuration variable carries, e.g. `MYAPP_`.
    prefix: String,
    /// What separates nesting levels in an environment key, e.g. `__`.
    separator: String,
    /// What marks a variable holding a path rather than a value, e.g. `_FILE`.
    file_suffix: String,
    /// Full environment spellings that a file may not supply. See [`Self::reserve`].
    reserved: BTreeSet<String>,
}

impl Dialect {
    /// A dialect over `prefix`, with `__` nesting and the `_FILE` suffix.
    ///
    /// The prefix is taken verbatim, trailing underscore included — `Dialect::new("MYAPP_")`,
    /// not `Dialect::new("MYAPP")`. Appending a separator here would be a guess about a
    /// convention the caller has already expressed.
    #[must_use]
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            separator: DEFAULT_SEPARATOR.to_owned(),
            file_suffix: DEFAULT_FILE_SUFFIX.to_owned(),
            reserved: BTreeSet::new(),
        }
    }

    /// Replace the nesting separator.
    ///
    /// Defaults to `__`.
    #[must_use]
    pub fn nesting_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Replace the file-indirection suffix.
    ///
    /// Defaults to `_FILE`.
    #[must_use]
    pub fn file_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.file_suffix = suffix.into();
        self
    }

    /// Reserve a key, in its **full environment spelling**, e.g. `MYAPP_PROFILE`.
    ///
    /// A reserved key is read straight from the environment, before or outside the layered
    /// config, so a file cannot supply it: naming one from a secrets directory or a `_FILE`
    /// variable is refused rather than ignored, because an ignored key is exactly the silent
    /// misconfiguration these layers exist to remove.
    ///
    /// Matching is **case-insensitive** — see [`Self::is_reserved`].
    #[must_use]
    pub fn reserve(mut self, key: impl Into<String>) -> Self {
        // Folded on the way in rather than on every comparison, so the set holds one spelling.
        self.reserved.insert(key.into().to_ascii_uppercase());
        self
    }

    /// The prefix every configuration variable carries.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// What separates nesting levels in an environment key.
    #[must_use]
    pub fn separator(&self) -> &str {
        &self.separator
    }

    /// What marks a variable holding a path rather than a value.
    ///
    /// Named for the concept rather than the field so it does not collide with the
    /// [`file_suffix`](Self::file_suffix) setter.
    #[must_use]
    pub fn indirection_suffix(&self) -> &str {
        &self.file_suffix
    }

    /// Whether `name` — a full environment spelling — is reserved.
    ///
    /// **Case-insensitive.** Environment variable names are case-insensitive on Windows, so an
    /// exact comparison makes the answer depend on how the operator happened to type the
    /// variable: `MYAPP_profile_FILE` and `MYAPP_PROFILE_FILE` are one variable there, and only
    /// the second would have been refused. It also left the two file layers disagreeing —
    /// [`SecretsDir`](crate::provider::SecretsDir) upper-cases a file name before this check
    /// while [`FileSuffixEnv`](crate::provider::FileSuffixEnv) had nothing to upper-case — so a
    /// key a secrets directory refused could still be supplied by indirection. Folding here
    /// makes every caller agree, and errs towards refusing, which is the safe direction for a
    /// check whose whole job is to refuse.
    #[must_use]
    pub fn is_reserved(&self, name: &str) -> bool {
        self.reserved.contains(&name.to_ascii_uppercase())
    }

    /// An environment key suffix (`AUTH__JWT_SECRET`) as a figment key path (`auth.jwt_secret`).
    ///
    /// The one spelling every layer uses, so a file and an environment variable cannot disagree
    /// about which field they name.
    ///
    /// **The separator is matched case-insensitively**, which for the default `__` is no
    /// difference at all and for a separator containing a letter is the whole of the function
    /// working. Both sides have to be folded because the two callers arrive in opposite cases: an
    /// environment suffix is `AUTH_X_JWT` by convention, and a secrets-directory file is named
    /// `auth_x_jwt`. Folding only the input — which is what this did — meant an upper-case
    /// separator never matched anything and nothing ever nested.
    ///
    /// That was not merely cosmetic. `Env::split` runs before figment lower-cases, so the
    /// environment layer *did* nest and the shadow check's view of the same variable did not:
    /// the two disagreed about every key, and
    /// [`ShadowPolicy::Reject`](crate::ShadowPolicy::Reject) silently stopped rejecting
    /// anything — the exact failure it exists to prevent.
    #[must_use]
    pub fn key_path(&self, suffix: &str) -> String {
        suffix
            .to_ascii_lowercase()
            .split(&self.separator.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(".")
    }

    /// A figment key path back in its environment spelling, for error messages.
    #[must_use]
    pub fn env_spelling(&self, key: &str) -> String {
        format!(
            "{}{}",
            self.prefix,
            key.to_ascii_uppercase().replace('.', &self.separator)
        )
    }

    /// The environment spelling of a bare key name, without treating `.` as nesting.
    ///
    /// A file named `profile` is spelled `MYAPP_PROFILE`.
    // One caller, the reserved check in `provider::secrets_dir`, which holds a file name rather
    // than a key path and so must not have `.` read as the nesting separator.
    pub(crate) fn env_spelling_of_name(&self, name: &str) -> String {
        format!("{}{}", self.prefix, name.to_ascii_uppercase())
    }

    /// The key an indirection variable names, if `name` is one.
    ///
    /// `<PREFIX><KEY><SUFFIX>`, with a **non-empty** `<KEY>`. That emptiness is the whole subtlety:
    /// with the default `_FILE`, `MYAPP_FILE` has nothing between the prefix and the suffix, so it
    /// is an ordinary key called `file` rather than an indirection naming a file.
    ///
    /// One definition with three callers — [`Self::plain_env_keys`],
    /// [`FileSuffixEnv`](crate::provider::FileSuffixEnv), and the schema's reachability check —
    /// because the cheaper `name.ends_with(suffix)` test they each used to carry disagreed with
    /// this one about exactly that case. A found-by-fuzzing disagreement: the environment layer
    /// supplied `file` from `MYAPP_FILE` while the shadow check could not see it, so a secrets
    /// file of the same name shadowed it silently.
    pub(crate) fn indirection_target<'a>(&self, name: &'a str) -> Option<&'a str> {
        name.strip_prefix(&self.prefix)
            .and_then(|key| key.strip_suffix(&self.file_suffix))
            .filter(|key| !key.is_empty())
    }

    /// Every key the environment supplies *directly*.
    ///
    /// Excludes the `_FILE` indirections, which are the mechanism rather than a value, and the
    /// reserved keys, which are not part of the layered config at all.
    pub(crate) fn plain_env_keys(&self) -> BTreeSet<String> {
        self.plain_env_entries().into_keys().collect()
    }

    /// Every key the environment supplies directly, against the variables that supply it.
    ///
    /// The set [`Self::plain_env_keys`] returns, with the spelling each key was found under kept
    /// rather than discarded — which is the whole of what an operator asking "where did this
    /// value come from" needs back.
    ///
    /// A key maps to a *set* of variables because more than one can produce it: environment
    /// names are case-sensitive on Linux and this dialect's key paths are not, so `MYAPP_PORT`
    /// and `MYAPP_port` are two variables and one key. Reporting both is the honest answer —
    /// figment reads both and one of them wins — and a set keeps the report deterministic where
    /// picking one out of `env::vars_os` order would not.
    pub(crate) fn plain_env_entries(&self) -> BTreeMap<String, BTreeSet<String>> {
        let mut keys: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (name, _) in std::env::vars_os() {
            let Some(name) = name.to_str() else { continue };
            if self.is_reserved(name) {
                continue;
            }
            // `auth.jwt_secret_file` is what the indirection variable would spell as a key, and
            // the environment layer withholds it for that reason. It must not be mistaken here
            // for `auth.jwt_secret`, which is what the indirection actually supplies.
            if self.indirection_target(name).is_some() {
                continue;
            }
            if let Some(suffix) = name.strip_prefix(&self.prefix)
                && !suffix.is_empty()
            {
                keys.entry(self.key_path(suffix))
                    .or_default()
                    .insert(name.to_owned());
            }
        }
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::Dialect;
    use crate::error::Error;
    use crate::terrace::Terrace;
    use crate::testing::Harness;

    /// What figment's own environment layer makes of the variables a jail has set.
    ///
    /// The comparison every test below is really making: this module's view of a variable has to
    /// be the view the layer that reads it has, or the shadow check compares two different
    /// spellings of every key and stops rejecting anything.
    fn env_layer(separator: &str) -> Result<figment::value::Value, Error> {
        figment::Figment::new()
            .merge(figment::providers::Env::prefixed("TEST_").split(separator))
            .extract()
            .map_err(|e| Error::from(Box::new(e)))
    }

    #[test]
    fn a_key_round_trips_between_its_two_spellings() {
        let dialect = Dialect::new("TEST_");
        assert_eq!(dialect.key_path("AUTH__JWT_SECRET"), "auth.jwt_secret");
        assert_eq!(
            dialect.env_spelling("auth.jwt_secret"),
            "TEST_AUTH__JWT_SECRET"
        );
    }

    /// A single-underscore separator must not split `JWT_SECRET` the way `__` leaves it alone.
    /// The separator is a parameter precisely because one deployment's `__` is another's `_`.
    #[test]
    fn a_custom_separator_changes_where_nesting_happens() {
        let dialect = Dialect::new("TEST_").nesting_separator("_");
        assert_eq!(dialect.key_path("AUTH_JWT"), "auth.jwt");
        assert_eq!(dialect.env_spelling("auth.jwt"), "TEST_AUTH_JWT");
    }

    #[test]
    fn reserving_is_by_full_environment_spelling() {
        let dialect = Dialect::new("TEST_").reserve("TEST_PROFILE");
        assert!(dialect.is_reserved("TEST_PROFILE"));
        assert!(!dialect.is_reserved("PROFILE"));
        assert!(!dialect.is_reserved("TEST_OTHER"));
    }

    /// A separator containing a letter used to break the round trip in one direction only:
    /// `key_path` folded the *input* to lower case and then looked for `_X_` in it, so nothing
    /// ever nested, while `env_spelling` substituted `_X_` verbatim going the other way.
    #[test]
    fn a_separator_containing_a_letter_round_trips() {
        let dialect = Dialect::new("TEST_").nesting_separator("_X_");
        assert_eq!(dialect.key_path("AUTH_X_JWT"), "auth.jwt");
        assert_eq!(dialect.env_spelling("auth.jwt"), "TEST_AUTH_X_JWT");
    }

    /// The two callers arrive in opposite cases: an environment suffix is upper-case by
    /// convention, and a secrets-directory file is named in whatever case the operator used. Both
    /// have to reach the same key or the layers stop agreeing about what they supply.
    #[test]
    fn a_separator_containing_a_letter_matches_in_either_case() {
        let dialect = Dialect::new("TEST_").nesting_separator("_X_");
        for spelling in ["AUTH_X_JWT", "auth_x_jwt", "Auth_X_jwt"] {
            assert_eq!(dialect.key_path(spelling), "auth.jwt", "{spelling}");
        }
    }

    /// The reason the above is a correctness bug rather than a cosmetic one. `Env::split` runs
    /// before figment folds case, so the environment layer nests on `_X_` regardless; if
    /// `plain_env_keys` does not, the shadow check compares two different spellings of every key
    /// and `ShadowPolicy::Reject` quietly stops rejecting.
    #[test]
    fn shadow_detection_sees_the_same_key_the_environment_layer_produces() {
        Harness::over(Terrace::new("TEST_").nesting_separator("_X_")).run(|jail| {
            jail.env("TEST_AUTH_X_JWT", "value");

            assert!(jail.dialect().plain_env_keys().contains("auth.jwt"));
            assert!(env_layer("_X_")?.find_ref("auth.jwt").is_some());
            Ok(())
        });
    }

    /// Found by the `schema` fuzz oracle. `TEST_FILE` *ends with* `_FILE` and is still an
    /// ordinary key called `file`: the indirection layer needs something between the prefix and
    /// the suffix, and the cheaper `ends_with` test every caller used to carry disagreed with it
    /// about exactly this name.
    #[test]
    fn a_name_that_is_only_the_prefix_and_the_suffix_is_not_an_indirection() {
        let dialect = Dialect::new("TEST_");
        assert_eq!(dialect.indirection_target("TEST_FILE"), None);
        assert_eq!(dialect.indirection_target("TEST_AUTH_FILE"), Some("AUTH"));
        // Neither is a name that merely ends in the letters.
        assert_eq!(dialect.indirection_target("TEST_PROFILE"), None);
        // Nor one without the prefix at all.
        assert_eq!(dialect.indirection_target("OTHER_AUTH_FILE"), None);
    }

    /// The consequence of the above, and the reason it is a correctness bug rather than a
    /// cosmetic one: `TEST_FILE` supplies the key `file` through the environment layer, so the
    /// shadow check has to see it there. While it did not, a secrets file of the same name
    /// shadowed it silently — which is the failure `ShadowPolicy::Reject` exists to prevent.
    #[test]
    fn a_key_named_for_the_suffix_is_still_seen_by_the_shadow_check() {
        Harness::new("TEST_").run(|jail| {
            jail.env("TEST_FILE", "value");

            assert!(jail.dialect().plain_env_keys().contains("file"));
            assert!(env_layer("__")?.find_ref("file").is_some());
            Ok(())
        });
    }

    /// Found by the `secrets_dir` fuzz oracle. An exact comparison let `TEST_profile_FILE`
    /// supply a key that a secrets-directory file named `profile` was refused — and on Windows
    /// those two variables are the same variable, so the outcome depended on how the operator
    /// typed it.
    #[test]
    fn reserving_ignores_case_in_both_directions() {
        let upper = Dialect::new("TEST_").reserve("TEST_PROFILE");
        assert!(upper.is_reserved("TEST_profile"));
        assert!(upper.is_reserved("test_Profile"));

        let lower = Dialect::new("TEST_").reserve("test_profile");
        assert!(lower.is_reserved("TEST_PROFILE"));
    }
}
