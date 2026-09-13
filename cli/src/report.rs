//! Findings, and how a run of them is rendered.
//!
//! Separated from the rules themselves so that a rule is a pure function of what it was handed: it
//! returns findings and decides nothing about where they are printed, whether the run fails, or
//! how a step summary is laid out. That is what lets every rule be tested by calling it and reading
//! the list back — no process, no cluster, no render — which is the shape the ~8,000 lines of tests
//! this half was ported from were already written in.
//!
//! # Two levels, and the distinction is not severity
//!
//! [`Level::Error`] is *found wrong*. [`Level::Warning`] is *could not check*: a secrets directory
//! mounted somewhere the file names cannot be read from, a variable unaccounted for under a
//! contract whose policy is `warn`, a `producer.loader` this build has no read table for. Silence
//! in those cases would be a gate reporting a success it has not earned, and an error would stop a
//! deployment this tool has no evidence against.

use std::fmt::Write as _;

/// Whether a finding is something found wrong, or something that could not be checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// The run failed: a rule was applied and the tree did not satisfy it.
    Error,
    /// The rule could not be applied. Reported, and does not fail the run.
    Warning,
}

impl Level {
    /// The spelling used in text output and in the JSON rendering.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// One thing a rule has to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Whether this fails the run.
    pub level: Level,
    /// What to tell the reader, whole. One finding is one line.
    pub message: String,
}

/// A finding that fails the run.
pub fn error(message: impl Into<String>) -> Finding {
    Finding {
        level: Level::Error,
        message: message.into(),
    }
}

/// A finding that reports something the rule could not check.
pub fn warning(message: impl Into<String>) -> Finding {
    Finding {
        level: Level::Warning,
        message: message.into(),
    }
}

/// One finding, tagged with what produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The chart or rendered file, then the document — so one line names the chart, the values
    /// file and the key without a reader opening anything.
    pub at: String,
    /// What was found.
    pub finding: Finding,
}

/// Every finding of one run, in the order it was found.
#[derive(Debug, Clone, Default)]
pub struct Report {
    entries: Vec<Entry>,
}

impl Report {
    /// An empty report.
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Record one finding.
    pub fn add(&mut self, at: impl Into<String>, finding: Finding) {
        self.entries.push(Entry {
            at: at.into(),
            finding,
        });
    }

    /// Record several findings under one location.
    pub fn extend(&mut self, at: &str, findings: impl IntoIterator<Item = Finding>) {
        for finding in findings {
            self.add(at, finding);
        }
    }

    /// Record one error.
    pub fn fail(&mut self, at: impl Into<String>, message: impl Into<String>) {
        self.add(at, error(message));
    }

    /// Every finding, in the order it was found.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// How many findings fail the run.
    pub fn error_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.finding.level == Level::Error)
            .count()
    }

    /// Whether anything failed the run.
    pub fn failed(&self) -> bool {
        self.error_count() > 0
    }

    /// The findings at one level, in the order they were found.
    pub fn at_level(&self, level: Level) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(move |entry| entry.finding.level == level)
    }

    /// The warnings, then the errors, as plain lines.
    ///
    /// Warnings first because they go to stdout and errors to stderr, and a reader scrolling back
    /// through a failed job wants the thing that failed it last rather than buried above the notes
    /// about what could not be checked.
    pub fn text(&self) -> (String, String) {
        let mut out = String::new();
        let mut errors = String::new();
        for entry in self.at_level(Level::Warning) {
            let _ = writeln!(out, "warning: {}: {}", entry.at, entry.finding.message);
        }
        for entry in self.at_level(Level::Error) {
            let _ = writeln!(errors, "{}: {}", entry.at, entry.finding.message);
        }
        (out, errors)
    }

    /// The whole report as JSON, which is what the parity harness and any other tool reads.
    ///
    /// # Panics
    /// Never: the value is built from owned strings, and `serde_json` cannot fail to serialise
    /// them.
    pub fn json(&self) -> String {
        let findings: Vec<serde_json::Value> = self
            .entries
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "where": entry.at,
                    "level": entry.finding.level.label(),
                    "message": entry.finding.message,
                })
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({ "findings": findings }))
            .expect("a report of owned strings serialises")
    }

    /// A table for a GitHub step summary, where a reviewer of a digest bump looks first.
    pub fn step_summary(&self, headline: &str, clean: &str) -> String {
        let mut lines = format!("## {headline}\n\n");
        if self.entries.is_empty() {
            lines.push_str(clean);
            lines.push('\n');
            return lines;
        }
        lines.push_str("| | Where | What |\n|---|---|---|\n");
        for entry in self
            .at_level(Level::Error)
            .chain(self.at_level(Level::Warning))
        {
            let icon = match entry.finding.level {
                Level::Error => "❌",
                Level::Warning => "⚠️",
            };
            let _ = writeln!(
                lines,
                "| {icon} | `{}` | {} |",
                entry.at,
                cell(&entry.finding.message)
            );
        }
        lines
    }
}

/// One message, safe to put inside a Markdown table cell.
fn cell(message: &str) -> String {
    message.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::{Level, Report, error, warning};

    #[test]
    fn a_warning_does_not_fail_the_run() {
        let mut report = Report::new();
        report.add("chart: doc", warning("could not check"));
        assert!(!report.failed());
        assert_eq!(report.error_count(), 0);
    }

    #[test]
    fn warnings_and_errors_go_to_different_streams() {
        let mut report = Report::new();
        report.add("a", warning("could not check"));
        report.add("b", error("found wrong"));
        let (out, errors) = report.text();
        assert_eq!(out, "warning: a: could not check\n");
        assert_eq!(errors, "b: found wrong\n");
    }

    #[test]
    fn the_json_rendering_carries_the_location_and_the_level() {
        let mut report = Report::new();
        report.add("a", error("found wrong"));
        let parsed: serde_json::Value =
            serde_json::from_str(&report.json()).expect("the report is JSON");
        assert_eq!(parsed["findings"][0]["where"], "a");
        assert_eq!(parsed["findings"][0]["level"], "error");
        assert_eq!(parsed["findings"][0]["message"], "found wrong");
    }

    #[test]
    fn a_pipe_in_a_message_does_not_break_the_summary_table() {
        let mut report = Report::new();
        report.add("a", error("a || b"));
        let summary = report.step_summary("Configuration contracts", "clean");
        assert!(summary.contains(r"a \|\| b"), "{summary}");
    }

    #[test]
    fn errors_come_before_warnings_in_the_summary() {
        let mut report = Report::new();
        report.add("first", warning("could not check"));
        report.add("second", error("found wrong"));
        let summary = report.step_summary("x", "clean");
        let error_row = summary.find("second").expect("the error is in the table");
        let warning_row = summary.find("first").expect("the warning is in the table");
        assert!(error_row < warning_row, "{summary}");
    }

    #[test]
    fn a_clean_run_says_so_rather_than_rendering_an_empty_table() {
        let report = Report::new();
        let summary = report.step_summary("x", "everything matches");
        assert!(summary.contains("everything matches"), "{summary}");
        assert!(!summary.contains("|---|"), "{summary}");
    }

    #[test]
    fn the_levels_are_ordered_so_a_sort_puts_errors_first() {
        assert!(Level::Error < Level::Warning);
    }
}
