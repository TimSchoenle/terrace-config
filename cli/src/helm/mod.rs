//! A chart repository's own conventions, above everything that is not one.
//!
//! `config-contract.yaml`, the vendored contracts under `charts/<chart>/contracts/`, the values tree
//! a chart pins its images in. None of it is part of the wire format, and none of it may leak
//! downward: `spec/v1/`'s `$id` is a promise that the directory describes `terrace_contract: 1` and
//! will not be edited to describe anything else, and the feature stack is the mechanical half of the
//! same promise. Everything here sits behind `helm`, above `k8s`, above the core — so
//! `--no-default-features` compiling is the check that no chart concept reached the document model.
//!
//! # It is self-describing, per chart
//!
//! A chart declares its own contracts and nothing central lists them, which is what makes adding a
//! chart free. A chart with no declaration is skipped; a chart with `documents: []` has opted out
//! explicitly and must say why, and that is the only permitted opt-out.

pub mod bindings;
pub mod check;
pub mod coverage;
pub mod declaration;
pub mod diff;
pub mod markers;
pub mod readme;
pub mod secrets;
pub mod shapes;
pub mod suites;
pub mod testgen;

pub use bindings::{Bindings, check as check_bindings};
pub use check::{Checked, check};
pub use coverage::{Coverage, coverage};
pub use declaration::{
    Binding, Bound, Consumer, Declaration, Document, ImageRef, Vendored, bind, declared,
    load_declaration, resolve_image,
};
pub use diff::{ChartDiff, Committed, Diffed, Revision, collect};
pub use markers::{Block, Class, Marker};
pub use secrets::{Credential, Surface, reconcile as reconcile_secrets};
pub use shapes::{Divergence, Shape};

use serde_json::Value as Json;

/// The file a chart declares its contracts in.
pub const DECLARATION: &str = "config-contract.yaml";

/// The chart tree, relative to a repository root.
pub const CHARTS_DIR: &str = "charts";

/// The list of image repositories an organisation builds, and so the ones that can be expected to
/// publish a contract.
pub const FIRST_PARTY: &str = ".github/configs/first-party-images.txt";

/// One value, spelled the way the reports this half was ported from spell one.
///
/// Only reached on a malformed declaration, where the message has to show what was written rather
/// than what it meant. Kept in this spelling for the same reason [`crate::gate`]'s quoting is: a
/// difference in a report should be a difference in a rule.
pub(crate) fn shown(value: Option<&Json>) -> String {
    match value {
        None | Some(Json::Null) => "None".to_owned(),
        Some(Json::Bool(true)) => "True".to_owned(),
        Some(Json::Bool(false)) => "False".to_owned(),
        Some(Json::String(text)) => crate::gate::quoted(text),
        Some(other) => other.to_string(),
    }
}

/// Follow a dotted values path, returning [`None`] at the first missing step.
///
/// Absence is an ordinary answer rather than an error: every caller is asking whether a chart
/// happens to declare something, and most charts do not declare most things.
pub fn dig<'a>(values: &'a Json, path: &str) -> Option<&'a Json> {
    let mut current = values;
    for part in path.split('.') {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

/// One release, with the `v` prefix that is spelled inconsistently across an estate removed.
///
/// A contract records the image's own tag; a chart's `appVersion` is the same release, carrying the
/// prefix in some charts and not in others. Both spellings are already committed, so normalising is
/// what comparing them can mean — the alternative is a gate reporting drift between `v8.9.1` and
/// `8.9.1`.
pub fn release(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix('v').unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{dig, release, shown};

    #[test]
    fn a_missing_step_is_an_ordinary_answer() {
        let values = json!({"image": {"repository": "ghcr.io/x/y"}});
        assert_eq!(
            dig(&values, "image.repository"),
            Some(&json!("ghcr.io/x/y"))
        );
        assert_eq!(dig(&values, "image.tag"), None);
        assert_eq!(dig(&values, "nothing.at.all"), None);
    }

    #[test]
    fn a_release_is_compared_without_its_prefix() {
        assert_eq!(release(" v8.9.1 "), "8.9.1");
        assert_eq!(release("8.9.1"), "8.9.1");
    }

    #[test]
    fn a_malformed_value_is_shown_as_it_was_written() {
        assert_eq!(shown(Some(&json!("yes"))), "'yes'");
        assert_eq!(shown(Some(&json!(true))), "True");
        assert_eq!(shown(None), "None");
        assert_eq!(shown(Some(&json!(1))), "1");
    }
}
