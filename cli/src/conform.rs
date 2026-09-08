//! What it means for a document to be one, and which tier its producer may claim.
//!
//! This is the whole of a new implementation's obligation. `spec/v1/` says what a contract is;
//! this says it back in a form a build can run, so a Java, Kotlin or Go producer answers "is what
//! I emitted a contract?" by invoking one binary instead of by re-reading prose and hoping.
//!
//! # Why this exists even though producers already refuse
//!
//! `FORMAT.md` requires a *producer* to fail rather than emit a document carrying any of the eight
//! refusals below, and both existing implementations do. That duplication is deliberate and the
//! roles are not symmetric:
//!
//! - **The producer's check is a fast path with better messages.** It knows the field, the
//!   annotation and the source position; this knows a JSON pointer.
//! - **This one is normative.** It is the same code for every producer, so "conforms" means one
//!   thing rather than one thing per language, and where the two disagree the producer is wrong.
//!
//! A producer that skipped its own check entirely would still be correct as long as its build runs
//! this — which is exactly the point for a language that has not been written yet.

use std::collections::BTreeSet;
use std::fmt;

use crate::document::{Contract, Key, Unreachable};

/// How much of the specification an implementation claims to meet.
///
/// The tiers exist because two implementations can agree completely about the *document* and still
/// disagree about the *spellings*, and can agree about both and still disagree about the *text*.
/// Collapsing them into one "conforms / does not" would either exclude every implementation
/// wrapping a different binder, or claim an interoperability nobody has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[non_exhaustive]
pub enum Tier {
    /// The document: well-formed, and satisfying every refusal.
    ///
    /// The floor, and enough for a consumer to validate a chart's rendered values, check
    /// `required` per key, and classify a container's variables. What a producer wrapping another
    /// framework's binder can reach without adopting any of terrace's naming.
    #[default]
    Document,
    /// Tier 1, and: the same key paths under the same dialect produce the same spellings.
    ///
    /// The tier worth aiming at, and the one that costs a decision. Deriving a variable name from
    /// a key path is not the hard part; agreeing about the cases where it *cannot* be derived is.
    Dialect,
    /// Tier 2, and: byte-identical output for a shared case.
    ///
    /// Reachable only between implementations sharing a loader *and* a type mapping. Not checkable
    /// from one document, so this level is accepted and reported rather than tested here — the
    /// corpus is what tests it.
    Byte,
}

impl Tier {
    /// The `--tier` spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Document => "1",
            Self::Dialect => "2",
            Self::Byte => "3",
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A `--tier` value that names no tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownTier(pub String);

impl fmt::Display for UnknownTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a tier. Try 1 (document), 2 (dialect) or 3 (byte).",
            self.0
        )
    }
}

impl std::error::Error for UnknownTier {}

impl std::str::FromStr for Tier {
    type Err = UnknownTier;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "1" | "document" => Ok(Self::Document),
            "2" | "dialect" => Ok(Self::Dialect),
            "3" | "byte" => Ok(Self::Byte),
            other => Err(UnknownTier(other.to_owned())),
        }
    }
}

/// One way a document is not the thing it claims to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Which rule, as `FORMAT.md` numbers them where it does.
    pub rule: &'static str,
    /// Where in the document, as a JSON pointer where one helps.
    pub at: String,
    /// What is wrong, in a sentence a producer's author can act on.
    pub detail: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} ({})", self.at, self.detail, self.rule)
    }
}

/// Hold a document to a tier.
///
/// Empty means it conforms. Tier 3 is accepted without further checks — byte-identity is a
/// property of two renderings of one case, not of one document, and the corpus is what tests it.
pub fn conform(contract: &Contract, tier: Tier) -> Vec<Violation> {
    let mut violations = refusals(contract);
    if tier >= Tier::Dialect {
        violations.extend(dialect(contract));
    }
    violations
}

/// The eight refusals: every way a contract could quietly stop being one.
///
/// Numbered as `FORMAT.md` numbers them, because a producer's author reading a failure here is
/// going to go and read that section, and a rule that has a number in one place and a name in the
/// other costs them the lookup.
fn refusals(contract: &Contract) -> Vec<Violation> {
    let prefix = &contract.schema.dialect.prefix;

    // Every spelling the loader itself reads. Not just the prefixed ones: the config and
    // secrets-directory variables take arbitrary names, which is what makes rules 3 and 4 more
    // than a restatement of 1 and 2.
    let loader: BTreeSet<&str> = contract
        .schema
        .loader
        .iter()
        .map(|var| var.env.as_str())
        .collect();

    let mut violations = Vec::new();
    // 7 first: everything below reasons about the namespace, and an empty prefix means there is
    // none to reason about. A prefixless loader cannot tell its own variables from the machine's,
    // so every other rule would pass vacuously against one.
    if prefix.is_empty() {
        violations.push(Violation {
            rule: "refusal 7",
            at: "/schema/dialect/prefix".to_owned(),
            detail: "the prefix is empty, so nothing distinguishes this loader's namespace from \
                     the machine's environment"
                .to_owned(),
        });
    }
    violations.extend(external_refusals(contract, prefix, &loader));
    violations.extend(ignore_refusals(contract, prefix, &loader));
    violations.extend(key_refusals(contract));
    violations
}

/// Refusals 1, 3, 5, and the external half of 6.
fn external_refusals(
    contract: &Contract,
    prefix: &str,
    loader_spellings: &BTreeSet<&str>,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();

    for (index, var) in contract.external.env.iter().enumerate() {
        let at = format!("/external/env/{index}");

        // 1
        if !prefix.is_empty() && var.name.starts_with(prefix) {
            violations.push(Violation {
                rule: "refusal 1",
                at: at.clone(),
                detail: format!(
                    "`{}` carries the loader's prefix `{prefix}`, which would leave it governed \
                     and exempt at once",
                    var.name
                ),
            });
        }

        // 3
        if loader_spellings.contains(var.name.as_str()) {
            violations.push(Violation {
                rule: "refusal 3",
                at: at.clone(),
                detail: format!(
                    "`{}` is a variable the loader itself reads, so declaring it external exempts \
                     the loader's own input",
                    var.name
                ),
            });
        }

        // 5
        if !seen.insert(var.name.as_str()) {
            violations.push(Violation {
                rule: "refusal 5",
                at: at.clone(),
                detail: format!(
                    "`{}` is declared twice; refusing beats picking one of two descriptions",
                    var.name
                ),
            });
        }

        // 6, on the external half
        if var.secret && var.default.is_some() {
            violations.push(Violation {
                rule: "refusal 6",
                at,
                detail: format!(
                    "`{}` is secret and carries a default, which publishes a credential in the one \
                     document meant to be safe to read",
                    var.name
                ),
            });
        }
    }

    violations
}

/// Refusals 2 and 4: the same exemption, through the other door.
fn ignore_refusals(
    contract: &Contract,
    prefix: &str,
    loader_spellings: &BTreeSet<&str>,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for (index, pattern) in contract.external.ignore.iter().enumerate() {
        let at = format!("/external/ignore/{index}");
        let covers = |name: &str| ignore_covers(pattern, name);

        // 2. Both doors: a pattern carrying the prefix, and one that merely subsumes it —
        // `PORT*` against `PORTFOLIO_` reads as a pattern about the external `PORT` and disables
        // the whole gate. An exact `PORT` is fine, which is why this asks about coverage rather
        // than about a shared leading substring.
        if !prefix.is_empty() && covers(prefix) {
            violations.push(Violation {
                rule: "refusal 2",
                at: at.clone(),
                detail: format!(
                    "`{pattern}` reaches into the loader's namespace `{prefix}`, exempting every \
                     key it happens to cover"
                ),
            });
        }

        // 4
        for spelling in loader_spellings {
            if covers(spelling) {
                violations.push(Violation {
                    rule: "refusal 4",
                    at: at.clone(),
                    detail: format!(
                        "`{pattern}` covers `{spelling}`, a variable the loader reads to decide \
                         what the layers are"
                    ),
                });
                break;
            }
        }
    }

    violations
}

/// Refusal 8, and the key half of 6.
fn key_refusals(contract: &Contract) -> Vec<Violation> {
    let mut violations = Vec::new();

    for (index, key) in contract.schema.keys.iter().enumerate() {
        let at = format!("/schema/keys/{index}");

        // 6, on the key half
        if key.secret && key.default_value.is_some() {
            violations.push(Violation {
                rule: "refusal 6",
                at: at.clone(),
                detail: format!(
                    "`{}` is secret and carries a default, which publishes a credential in the one \
                     document meant to be safe to read",
                    key.path
                ),
            });
        }

        // 8. The producer must refuse to *build* this; a document carrying it describes an effect
        // no consumer can express, so every gate downstream would pass on it.
        if key.unreachable == Some(Unreachable::Indirection) {
            violations.push(Violation {
                rule: "refusal 8",
                at,
                detail: format!(
                    "`{}` is unreachable through another key's indirection variable, which is a \
                     shape a producer must refuse rather than publish",
                    key.path
                ),
            });
        }
    }

    violations
}

/// Tier 2: every spelling the document states is the one the dialect derives.
///
/// The check a tier 1 implementation is *expected* to fail, which is why it is a separate tier and
/// not a refusal. An implementation handing naming to a binder with its own relaxed-binding rules
/// will not reach this without overriding it, and should say tier 1 rather than pretend.
fn dialect(contract: &Contract) -> Vec<Violation> {
    let dialect = &contract.schema.dialect;
    let mut violations = Vec::new();

    for (index, key) in contract.schema.keys.iter().enumerate() {
        let at = format!("/schema/keys/{index}");
        let derived = derive_env(&key.path, &dialect.prefix, &dialect.nesting_separator);

        match (&key.env, &derived) {
            // A key the environment cannot name must say so, and the reason has to be one of the
            // two: a bare `null` is read as "skip this key", which is right for `unnameable` and
            // wrong for `indirection`.
            (None, _) if key.unreachable.is_none() => violations.push(Violation {
                rule: "tier 2",
                at: at.clone(),
                detail: format!(
                    "`{}` has no environment spelling and no `unreachable` saying why. A consumer \
                     meeting a bare null treats it as \"skip this key\", which is wrong for one of \
                     the two reasons.",
                    key.path
                ),
            }),
            (Some(env), Some(derived)) if env != derived => violations.push(Violation {
                rule: "tier 2",
                at: at.clone(),
                detail: format!(
                    "`{}` is spelled `{env}`; this dialect derives `{derived}`",
                    key.path
                ),
            }),
            (Some(env), None) => violations.push(Violation {
                rule: "tier 2",
                at: at.clone(),
                detail: format!(
                    "`{}` is spelled `{env}`, but no name derives from this path — it does not \
                     survive the case fold, or it carries the nesting separator",
                    key.path
                ),
            }),
            // Either the spelling is the one this dialect derives, or the key is unspelled and has
            // already said which of the two reasons applies.
            (None, _) | (Some(_), Some(_)) => {}
        }

        // The indirection spelling is the environment one plus the suffix, and nothing else.
        if let (Some(env), Some(env_file)) = (&key.env, &key.env_file) {
            let expected = format!("{env}{}", dialect.indirection_suffix);
            if env_file != &expected {
                violations.push(Violation {
                    rule: "tier 2",
                    at,
                    detail: format!(
                        "`{}` names its indirection variable `{env_file}`; this dialect derives \
                         `{expected}`",
                        key.path
                    ),
                });
            }
        }
    }

    violations
}

/// The environment spelling a path derives, or `None` when none does.
///
/// The two cases that produce `None` are the ones tier 2 exists to make implementations agree
/// about: a path that does not survive the case fold, and one already carrying the separator.
fn derive_env(path: &str, prefix: &str, separator: &str) -> Option<String> {
    if path.contains(separator) {
        return None;
    }
    let upper = path.replace('.', separator).to_uppercase();
    // Round-tripped rather than assumed: a path is nameable exactly when lowercasing the name
    // gives the path back, which is what a consumer reading the variable has to be able to do.
    let back = upper.to_lowercase().replace(separator, ".");
    if back != path {
        return None;
    }
    Some(format!("{prefix}{upper}"))
}

/// Whether an ignore pattern covers a name.
///
/// A single trailing `*` is the whole of the pattern syntax — deliberately, since the document
/// carries as little of a language two implementations could disagree in as it can.
fn ignore_covers(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(stem) => name.starts_with(stem),
        None => pattern == name,
    }
}

/// Every key whose `required` a consumer must satisfy from some layer.
///
/// Not a conformance rule — a convenience for the consumer half, which asks this of every document
/// it gates against and would otherwise each write the filter.
pub fn required_keys(contract: &Contract) -> impl Iterator<Item = &Key> {
    contract.schema.keys.iter().filter(|key| key.required)
}

#[cfg(test)]
mod tests {
    use super::{Tier, derive_env, ignore_covers};

    #[test]
    fn a_pattern_subsuming_the_prefix_is_caught_and_an_exact_name_is_not() {
        // The case FORMAT.md calls out by name: `PORT*` against `PORTFOLIO_` reads as a pattern
        // about the external `PORT` and disables the whole gate.
        assert!(ignore_covers("PORT*", "PORTFOLIO_"));
        assert!(!ignore_covers("PORT", "PORTFOLIO_"));
        assert!(ignore_covers("KUBERNETES_*", "KUBERNETES_SERVICE_HOST"));
    }

    #[test]
    fn a_path_that_does_not_survive_the_case_fold_names_nothing() {
        assert_eq!(
            derive_env("github.token", "P_", "__").as_deref(),
            Some("P_GITHUB__TOKEN")
        );
        // camelCase: upper-casing loses the boundary, so lower-casing cannot give it back.
        assert_eq!(derive_env("distDir", "P_", "__"), None);
        // Already carrying the separator.
        assert_eq!(derive_env("a__b", "P_", "__"), None);
    }

    #[test]
    fn the_tiers_are_ordered_so_a_higher_one_includes_a_lower() {
        assert!(Tier::Byte > Tier::Dialect);
        assert!(Tier::Dialect > Tier::Document);
    }
}
