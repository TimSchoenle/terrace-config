//! `adopt`, through the binary: the exit codes a caller tells its outcomes apart by.
//!
//! The planning and the writing are tested beside the code in `helm::adopt`. What only the binary
//! can show is which code a failure leaves with, and that depends on where in the handler the
//! failure happens. The handler reads the chart tree inside its own error path, so a tree that
//! cannot be read exits 1 by default and with `--diff` or `--write`, and 2 only with `--check`;
//! it never reaches `main`, which would exit 2 in every mode.

#![cfg(feature = "cli")]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The variables that would make the binary read the caller's configuration instead of the test's.
const INHERITED: [&str; 3] = ["TERRACE_CONFIG", "TERRACE_STRICT", "TERRACE_CHARTS"];

/// An empty scratch directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "terrace-adopt-bin-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch directory");
        // A terrace.yaml of its own, so the binary never walks up into the repository it runs in.
        std::fs::write(root.join("terrace.yaml"), "version: 1\n").expect("a settings file");
        Self(root)
    }

    fn adopt(&self, arguments: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_terrace-contract"));
        for name in INHERITED {
            command.env_remove(name);
        }
        command
            .current_dir(&self.0)
            .arg("--config")
            .arg(self.0.join("terrace.yaml"))
            .arg("adopt")
            .args(arguments)
            .output()
            .expect("the binary runs")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn shown(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_tree_that_cannot_be_read_exits_one_and_two_only_under_check() {
    let scratch = Scratch::new();
    for (mode, expected) in [
        (None, 1),
        (Some("--diff"), 1),
        (Some("--write"), 1),
        (Some("--check"), 2),
    ] {
        let mut arguments = vec!["--charts", "missing"];
        arguments.extend(mode);
        let output = scratch.adopt(&arguments);
        assert_eq!(
            output.status.code(),
            Some(expected),
            "{mode:?}: {}",
            shown(&output)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with("error: "),
            "{mode:?}: {}",
            shown(&output)
        );
    }
}

#[test]
fn a_selection_that_names_no_chart_exits_two_in_every_mode() {
    let scratch = Scratch::new();
    std::fs::create_dir_all(scratch.0.join("charts")).expect("an empty tree");
    for mode in [None, Some("--diff"), Some("--write"), Some("--check")] {
        let mut arguments = vec!["nope", "--charts", "charts"];
        arguments.extend(mode);
        let output = scratch.adopt(&arguments);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{mode:?}: {}",
            shown(&output)
        );
        assert!(shown(&output).contains("error: charts/nope is not a chart"));

        let mut arguments = vec!["--charts", "charts", "--key", "a.b"];
        arguments.extend(mode);
        let output = scratch.adopt(&arguments);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{mode:?}: {}",
            shown(&output)
        );
    }
}

#[test]
fn a_tree_with_nothing_owed_exits_zero() {
    let scratch = Scratch::new();
    std::fs::create_dir_all(scratch.0.join("charts")).expect("an empty tree");
    let output = scratch.adopt(&["--charts", "charts"]);
    assert_eq!(output.status.code(), Some(0), "{}", shown(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "==> every contract key of every enrolled chart is bound or written off\n"
    );
    let output = scratch.adopt(&["--charts", "charts", "--check"]);
    assert_eq!(output.status.code(), Some(0), "{}", shown(&output));
}
