//! How a deployment spells a configuration key in its environment.
//!
//! Every name this crate reads or reports is derived from one [`Dialect`]: the prefix, the
//! nesting separator, the `_FILE` suffix, and the handful of keys that are read before the
//! layers exist. It is the whole of the parameterisation that turned a project-specific loader
//! into a crate — in the original there were four `const`s here and a hardcoded array.

use std::collections::BTreeSet;

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

    /// Replace the nesting separator. Defaults to `__`.
    #[must_use]
    pub fn nesting_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Replace the file-indirection suffix. Defaults to `_FILE`.
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
    #[must_use]
    pub fn key_path(&self, suffix: &str) -> String {
        suffix
            .to_ascii_lowercase()
            .split(&self.separator)
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
    /// Used to test a secrets-directory file name against the reserved set: the file is named
    /// `profile`, and what must be checked is whether `MYAPP_PROFILE` is reserved.
    pub(crate) fn env_spelling_of_name(&self, name: &str) -> String {
        format!("{}{}", self.prefix, name.to_ascii_uppercase())
    }

    /// Every key the environment supplies *directly*.
    ///
    /// Excludes the `_FILE` indirections, which are the mechanism rather than a value, and the
    /// reserved keys, which are not part of the layered config at all.
    pub(crate) fn plain_env_keys(&self) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for (name, _) in std::env::vars_os() {
            let Some(name) = name.to_str() else { continue };
            if self.is_reserved(name) {
                continue;
            }
            // `auth.jwt_secret_file` is what figment makes of the indirection variable — an
            // unknown key it ignores. It must not be mistaken for `auth.jwt_secret`, which is
            // what the indirection actually supplies.
            if name.ends_with(&self.file_suffix) {
                continue;
            }
            if let Some(suffix) = name.strip_prefix(&self.prefix)
                && !suffix.is_empty()
            {
                keys.insert(self.key_path(suffix));
            }
        }
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::Dialect;

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
