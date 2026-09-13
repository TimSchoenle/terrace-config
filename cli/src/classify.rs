//! What one environment variable is. First match wins, and the order is normative.
//!
//! Two consumers running these steps in different orders disagree about whether a deployment is
//! valid, which is the whole reason the list is written down. Step 4 sitting *above* steps 5 and 6
//! is the load-bearing part: it stops either external list from exempting a variable inside the
//! loader's own namespace. A producer refuses to build a contract that tries, and this order is the
//! consumer-side half of the same guarantee.
//!
//! Nothing here derives a spelling. Every step reads `env`, `env_file`, `prefix` and the external
//! lists exactly as the document published them, which is tier 1 work and is why a producer that
//! never reaches tier 2 is still usable by every rule that reads this.

use crate::union::{Merged, Union};

/// What one variable turned out to be, named after the step that decided it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 1. A variable the loader reads to decide what the layers are.
    Loader,
    /// 2. A key, supplied by the environment layer.
    KeyEnv,
    /// 3. A key, by `_FILE` indirection.
    KeyEnvFile,
    /// 4. Inside the namespace and spelling nothing the contract names.
    Prefixed,
    /// 5. Declared as read by this image but owned elsewhere.
    External,
    /// 6. Matched an ignore pattern.
    Ignored,
    /// 7. Everything else, and what to do about it is `external.unknown`.
    Unknown,
}

/// What one variable is, and the entry that says so.
#[derive(Debug, Clone, Copy)]
pub struct Classification<'a> {
    /// Which step matched.
    pub kind: Kind,
    /// The key, loader variable or external variable that matched, when one did.
    pub entry: Option<&'a Merged>,
}

/// Decide what one environment variable is.
pub fn classify<'a>(union: &'a Union, name: &str) -> Classification<'a> {
    if let Some(entry) = union.loader.get(name) {
        return Classification {
            kind: Kind::Loader,
            entry: Some(entry),
        };
    }
    if let Some(entry) = union.key_by("env", name) {
        return Classification {
            kind: Kind::KeyEnv,
            entry: Some(entry),
        };
    }
    if let Some(entry) = union.key_by("env_file", name) {
        return Classification {
            kind: Kind::KeyEnvFile,
            entry: Some(entry),
        };
    }
    if name.starts_with(union.prefix()) {
        return Classification {
            kind: Kind::Prefixed,
            entry: None,
        };
    }
    if let Some(entry) = union.external_env.get(name) {
        return Classification {
            kind: Kind::External,
            entry: Some(entry),
        };
    }
    if union
        .ignore
        .iter()
        .any(|pattern| matches_ignore(pattern, name))
    {
        return Classification {
            kind: Kind::Ignored,
            entry: None,
        };
    }
    Classification {
        kind: Kind::Unknown,
        entry: None,
    }
}

/// The whole ignore pattern language: a trailing `*` matches any suffix, otherwise exact.
///
/// Deliberately not a glob and deliberately not a regex. A producer refuses any pattern that reaches
/// into the loader's namespace, and it can only make that judgement about a language small enough to
/// reason about — `ignore("PORT*")` carries no prefix, reads as a pattern about an external `PORT`,
/// and would subsume a `PORTFOLIO_` namespace entirely.
pub fn matches_ignore(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(opening) => name.starts_with(opening),
        None => name == pattern,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value as Json, json};

    use super::{Kind, classify, matches_ignore};
    use crate::union::{Union, union_contracts};

    fn union(keys: &Json, loader: &Json, external: &Json, ignore: &Json) -> Union {
        let contract = json!({
            "terrace_contract": 1,
            "producer": {"name": "x", "version": "1", "loader": "figment"},
            "app": {"name": "x"},
            "schema": {
                "schema_version": 2,
                "dialect": {"prefix": "P_", "nesting_separator": "__", "indirection_suffix": "_FILE"},
                "loader": loader.clone(), "keys": keys.clone(),
            },
            "json_schema": {},
            "external": {"env": external.clone(), "ignore": ignore.clone(), "unknown": "reject"},
        });
        union_contracts(&[("a".to_owned(), contract)]).expect("one contract merges")
    }

    #[test]
    fn the_steps_run_in_the_order_the_format_states() {
        let merged = union(
            &json!([{"path": "a", "env": "P_A", "env_file": "P_A_FILE", "text_form": "text"}]),
            &json!([{"env": "P_CONFIG", "role": "config"}]),
            &json!([{"name": "PORT", "text_form": "integer"}]),
            &json!(["RUST_LOG"]),
        );
        assert_eq!(classify(&merged, "P_CONFIG").kind, Kind::Loader);
        assert_eq!(classify(&merged, "P_A").kind, Kind::KeyEnv);
        assert_eq!(classify(&merged, "P_A_FILE").kind, Kind::KeyEnvFile);
        assert_eq!(classify(&merged, "P_SOMETHING").kind, Kind::Prefixed);
        assert_eq!(classify(&merged, "PORT").kind, Kind::External);
        assert_eq!(classify(&merged, "RUST_LOG").kind, Kind::Ignored);
        assert_eq!(classify(&merged, "HOME").kind, Kind::Unknown);
    }

    #[test]
    fn neither_external_list_can_exempt_a_variable_inside_the_namespace() {
        // Step 4 sits above steps 5 and 6 for exactly this. A producer refuses to build a contract
        // that tries; this is the consumer-side half of the same guarantee.
        let merged = union(
            &json!([]),
            &json!([]),
            &json!([{"name": "P_SNEAKY", "text_form": "text"}]),
            &json!(["P_*"]),
        );
        assert_eq!(classify(&merged, "P_SNEAKY").kind, Kind::Prefixed);
        assert_eq!(classify(&merged, "P_ANYTHING").kind, Kind::Prefixed);
    }

    #[test]
    fn the_pattern_language_is_a_trailing_star_and_nothing_else() {
        assert!(matches_ignore("RUST_LOG", "RUST_LOG"));
        assert!(!matches_ignore("RUST_LOG", "RUST_LOG_STYLE"));
        assert!(matches_ignore("RUST_*", "RUST_LOG"));
        assert!(!matches_ignore("RUST_?OG", "RUST_LOG"));
    }
}
