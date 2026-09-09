//! The three gates, and the scopes that make them three rather than one.
//!
//! ```text
//! gate 1  the rendered document   against the union of every contract that reads it
//! gate 2  one container's env     against the one image that container runs
//! gate 3  one container's mounts  against the one image that container runs
//! ```
//!
//! **The scopes differ, and the difference is the point.** A document is a file the format lets any
//! number of images read, so validating it against one of their contracts with
//! `additionalProperties: false` would call every other image's keys unknown and reject a correct
//! deployment. A container runs exactly one image, so a variable set on it that only a sibling image
//! reads is precisely the defect gate 2 exists to catch — and checking that against the merged
//! contract reintroduces what splitting the scopes removed.
//!
//! Every gate is a function of what it was handed and returns findings. It decides nothing about
//! where they are printed or whether the run fails, which is what lets a test construct a manifest
//! and a contract and read the list back.

pub mod container;
pub mod document;

pub use container::{ServiceLinks, check_container};
pub use document::{DocumentFormat, DocumentSource, check_document};

/// Which gates one rendered values file is exempt from.
///
/// Typed rather than a set of strings, because the four names are a closed vocabulary and a caller
/// that misspelled one would otherwise get a silently unrelaxed gate — the failure mode an
/// exemption exists to avoid being ignored in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Four independent switches, one per gate an exemption may name. A bitflag set would turn `relaxed.env` — which every rule reads — into a lookup, and the vocabulary is closed: it will not grow with the number of gates."
)]
pub struct Relaxed {
    /// Gate 1 entirely: the rendered document is not validated.
    pub document: bool,
    /// Only gate 1's `additionalProperties: false`: unknown keys are tolerated.
    ///
    /// The one case that needs it is a chart whose values append verbatim text to the rendered
    /// document, which the chart never parses — so keys it introduces are invisible to the renderer
    /// that would otherwise have to declare them.
    pub closed: bool,
    /// Gate 2: the container environment is not classified or checked.
    pub env: bool,
    /// Gate 3: secret file names and `_FILE` targets are not checked.
    pub files: bool,
}

impl Relaxed {
    /// The gate names an exemption may carry, and what each one drops.
    pub const GATES: [(&'static str, &'static str); 4] = [
        (
            "closed",
            "only gate 1's `additionalProperties: false`: unknown keys are tolerated",
        ),
        (
            "document",
            "gate 1 entirely: the rendered document is not validated",
        ),
        (
            "env",
            "gate 2: the container environment is not classified or checked",
        ),
        (
            "files",
            "gate 3: secret file names and `_FILE` targets are not checked",
        ),
    ];

    /// Relax one gate by name, or [`None`] when the name is not one of the four.
    pub fn relax(&mut self, gate: &str) -> Option<()> {
        match gate {
            "document" => self.document = true,
            "closed" => self.closed = true,
            "env" => self.env = true,
            "files" => self.files = true,
            _ => return None,
        }
        Some(())
    }
}

/// One string, quoted the way the reports this half was ported from quote one.
///
/// Deliberately not [`std::fmt::Debug`]'s spelling. Every message here names a key path, a file name
/// or a variable, and the corpus of expected output those messages were written against uses single
/// quotes — so keeping the spelling is what makes a difference in a report a difference in a rule.
pub(crate) fn quoted(text: &str) -> String {
    if text.contains('\'') && !text.contains('"') {
        format!("\"{text}\"")
    } else {
        format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

/// One value, quoted the way JSON quotes it.
pub(crate) fn json_text(text: &str) -> String {
    serde_json::Value::String(text.to_owned()).to_string()
}

#[cfg(test)]
mod tests {
    use super::{Relaxed, json_text, quoted};

    #[test]
    fn a_gate_name_outside_the_four_is_refused_rather_than_ignored() {
        let mut relaxed = Relaxed::default();
        assert!(relaxed.relax("env").is_some());
        assert!(relaxed.relax("gate2").is_none());
        assert!(relaxed.env);
        assert!(!relaxed.files);
    }

    #[test]
    fn quoting_matches_the_spelling_the_reports_were_written_against() {
        assert_eq!(quoted("server.port"), "'server.port'");
        assert_eq!(quoted("it's"), "\"it's\"");
        assert_eq!(json_text("a\"b"), "\"a\\\"b\"");
    }
}
