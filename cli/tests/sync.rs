//! `scope`, `sync` and `diff`, through the binary, against a real Git repository: which charts a
//! change selects, which steps a run takes, which revision a comparison reads, and which exit code a
//! pipeline sees. Also what the gate steps a run takes print when they examine nothing.

#![cfg(feature = "cli")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value as Json;

/// What a test process must not inherit: Git's repository-locating variables, which a hook exports
/// and which would point every command at the caller's repository, and the variables that would
/// make the binary read the caller's configuration or write into the caller's job summary.
const INHERITED: [&str; 12] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_PREFIX",
    "TERRACE_CONFIG",
    "TERRACE_STRICT",
    "TERRACE_CHARTS",
    "CONTRACT_SIGNER",
    "GITHUB_STEP_SUMMARY",
];

/// `command` without anything [`INHERITED`] names.
fn isolated(mut command: Command) -> Command {
    for name in INHERITED {
        command.env_remove(name);
    }
    command
}

/// A Git repository in the temporary directory with one commit on `main`, removed when dropped.
struct Repository(PathBuf);

impl Repository {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "terrace-sync-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let repository = Self(root);
        repository.git(&["init", "--quiet", "--initial-branch=main"]);
        repository
            .write("terrace.yaml", "version: 1\n")
            .chart("web", "application")
            .chart("api", "application")
            .chart("common", "library");
        repository.commit("base");
        repository
    }

    fn git(&self, args: &[&str]) {
        let output = isolated(Command::new("git"))
            .current_dir(&self.0)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?}: {}", text(&output));
    }

    fn write(&self, relative: &str, text: &str) -> &Self {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(path, text).expect("write");
        self
    }

    fn chart(&self, name: &str, kind: &str) -> &Self {
        self.write(
            &format!("charts/{name}/Chart.yaml"),
            &format!("apiVersion: v2\nname: {name}\ntype: {kind}\nversion: 1.0.0\n"),
        )
        .write(&format!("charts/{name}/values.yaml"), "replicas: 1\n")
    }

    /// Commit every change on a new branch, so `--since main` measures this commit alone.
    fn branch_with(&self, relative: &str, text: &str) {
        self.git(&["checkout", "--quiet", "-B", "change", "main"]);
        self.write(relative, text);
        self.commit("change");
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "-m", message]);
    }

    fn run(&self, args: &[&str]) -> Output {
        isolated(Command::new(env!("CARGO_BIN_EXE_terrace-contract")))
            .current_dir(&self.0)
            .args(args)
            .output()
            .expect("the binary runs")
    }

    fn scope(&self, since: &str) -> Json {
        let output = self.run(&["scope", "--since", since, "--json"]);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).expect("scope --json prints JSON")
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exited")
}

#[test]
fn scope_narrows_to_the_application_chart_a_change_touched() {
    let repository = Repository::new();
    repository.branch_with("charts/web/values.yaml", "replicas: 2\n");

    let shown = repository.scope("main");
    assert_eq!(shown["every"], false);
    assert_eq!(shown["charts"], serde_json::json!(["web"]));

    let output = repository.run(&["scope", "--since", "main"]);
    assert_eq!(code(&output), 0, "{}", text(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "web\n");
}

#[test]
fn scope_widens_on_a_change_outside_the_charts() {
    let repository = Repository::new();
    repository.branch_with("terrace.yaml", "version: 1\ncli: \"0.9\"\n");

    let shown = repository.scope("main");
    assert_eq!(shown["every"], true);
    assert_eq!(shown["charts"], serde_json::json!([]));
    assert!(
        shown["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("terrace.yaml")),
        "{shown}"
    );

    // The line form prints an empty line, which every gate reads as every chart, and says why on
    // stderr.
    let output = repository.run(&["scope", "--since", "main"]);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("every chart"));
}

#[test]
fn scope_widens_on_a_library_chart_and_on_an_unknown_revision() {
    let repository = Repository::new();
    repository.branch_with("charts/common/values.yaml", "replicas: 3\n");
    assert_eq!(repository.scope("main")["every"], true);

    let unknown = repository.scope("no-such-revision");
    assert_eq!(unknown["every"], true);
    assert!(
        unknown["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("could not be read")),
        "{unknown}"
    );
}

#[test]
fn scope_widens_on_a_chart_that_was_a_library_at_the_base() {
    // Deleted: the charts that depended on it are what break.
    let repository = Repository::new();
    repository.git(&["checkout", "--quiet", "-B", "change", "main"]);
    repository.git(&["rm", "--quiet", "-r", "charts/common"]);
    repository.write("charts/web/values.yaml", "replicas: 2\n");
    repository.commit("drop the library");
    assert_eq!(repository.scope("main")["every"], true);

    // Turned into an application chart: its dependents were built against the library.
    let repository = Repository::new();
    repository.branch_with(
        "charts/common/Chart.yaml",
        "apiVersion: v2\nname: common\ntype: application\nversion: 1.0.0\n",
    );
    assert_eq!(repository.scope("main")["every"], true);
}

#[test]
fn scope_leaves_out_a_deleted_application_chart() {
    let repository = Repository::new();
    repository.git(&["checkout", "--quiet", "-B", "change", "main"]);
    repository.git(&["rm", "--quiet", "-r", "charts/api"]);
    repository.write("charts/web/values.yaml", "replicas: 2\n");
    repository.commit("drop api");
    assert_eq!(
        repository.scope("main")["charts"],
        serde_json::json!(["web"])
    );
}

#[test]
fn scope_reads_a_path_git_would_otherwise_quote() {
    let repository = Repository::new();
    repository.branch_with("charts/web/files/größe.txt", "x\n");
    assert_eq!(
        repository.scope("main")["charts"],
        serde_json::json!(["web"])
    );
}

#[test]
fn sync_check_runs_every_offline_gate_and_passes_on_a_tree_without_contracts() {
    let repository = Repository::new();
    let output = repository.run(&["sync", "--check"]);
    let shown = text(&output);
    assert_eq!(code(&output), 0, "{shown}");
    for step in ["bindings", "shapes", "readme", "tests"] {
        assert!(
            shown.contains(&format!("==> sync: {step}")),
            "{step}: {shown}"
        );
    }
    // No first-party list is configured, so coverage has no question to answer.
    assert!(shown.contains("coverage skipped"), "{shown}");
    assert!(!shown.contains("==> sync: check"), "{shown}");
}

#[test]
fn sync_check_runs_coverage_once_first_party_images_are_named() {
    let repository = Repository::new();
    repository.write(
        "terrace.yaml",
        "version: 1\nhelm:\n  firstParty: [ghcr.io/example/*]\n",
    );
    let output = repository.run(&["sync", "--check"]);
    let shown = text(&output);
    assert_eq!(code(&output), 0, "{shown}");
    assert!(shown.contains("==> sync: coverage"), "{shown}");
}

#[test]
fn sync_check_since_a_revision_selects_the_charts_it_touched() {
    let repository = Repository::new();
    repository.branch_with("charts/api/values.yaml", "replicas: 2\n");
    let output = repository.run(&["sync", "--check", "--since", "main"]);
    let shown = text(&output);
    assert_eq!(code(&output), 0, "{shown}");
    assert!(shown.contains("==> sync: api\n"), "{shown}");
}

#[test]
fn sync_check_with_manifests_requires_the_render_to_exist() {
    let repository = Repository::new();
    let output = repository.run(&["sync", "--check", "--manifests", "rendered"]);
    let shown = text(&output);
    // The `check` step could not run, which is 2, while every other step still ran.
    assert_eq!(code(&output), 2, "{shown}");
    assert!(shown.contains("no rendered manifests"), "{shown}");
    assert!(shown.contains("==> sync: tests"), "{shown}");

    std::fs::create_dir_all(repository.path().join("rendered")).expect("render dir");
    let output = repository.run(&["sync", "--check", "--manifests", "rendered"]);
    assert_eq!(code(&output), 0, "{}", text(&output));
}

#[test]
fn sync_refuses_a_chart_the_tree_does_not_hold_before_any_step_runs() {
    let repository = Repository::new();
    let output = repository.run(&["sync", "--no-pull", "nope"]);
    let shown = text(&output);
    assert_eq!(code(&output), 2, "{shown}");
    assert!(!shown.contains("==> sync:"), "{shown}");
}

#[test]
fn sync_writes_from_the_vendored_contracts_without_pull() {
    let repository = Repository::new();
    let output = repository.run(&["sync", "--no-pull"]);
    let shown = text(&output);
    assert_eq!(code(&output), 0, "{shown}");
    assert!(!shown.contains("==> sync: pull"), "{shown}");
    assert!(shown.contains("==> sync: shapes"), "{shown}");
    assert!(shown.contains("==> sync: readme"), "{shown}");
    assert!(!shown.contains("==> sync: tests"), "{shown}");
}

#[test]
fn sync_refuses_flags_that_contradict_each_other() {
    let repository = Repository::new();
    for args in [
        &["sync", "--format", "json"][..],
        &["sync", "--manifests", "rendered"][..],
        &["sync", "--check", "--no-pull"][..],
        &["sync", "web", "--since", "main"][..],
    ] {
        let output = repository.run(args);
        assert_eq!(code(&output), 2, "{args:?}: {}", text(&output));
    }
}

#[test]
fn sync_in_github_format_groups_each_step() {
    let repository = Repository::new();
    let output = repository.run(&["sync", "--check", "--format", "github"]);
    let shown = String::from_utf8_lossy(&output.stdout);
    assert_eq!(code(&output), 0, "{}", text(&output));
    assert!(shown.contains("::group::sync: shapes"), "{shown}");
    assert_eq!(
        shown.matches("::group::").count(),
        shown.matches("::endgroup::").count()
    );
}

/// A vendored contract whose only moving part is the app version.
fn vendored(app_version: &str) -> String {
    serde_json::json!({
        "source": {"image": "i", "digest": "sha256:0", "sha256": "a", "fetched": "t"},
        "contract": {
            "terrace_contract": 1,
            "app": {"name": "x", "version": app_version},
            "schema": {"schema_version": 2, "dialect": {}, "loader": [], "keys": []},
            "json_schema": {},
            "external": {"env": [], "ignore": [], "unknown": "reject"},
        },
    })
    .to_string()
}

#[test]
fn diff_reads_the_base_revision_of_a_chart_tree_below_the_repository_root() {
    // The second parent has a byte Git prints quoted unless asked not to.
    for parent in ["deploy", "déploiement"] {
        let repository = Repository::new();
        repository
            .write(
                &format!("{parent}/terrace.yaml"),
                "version: 1\nhelm:\n  charts: charts\n",
            )
            .write(
                &format!("{parent}/charts/web/Chart.yaml"),
                "apiVersion: v2\nname: web\nversion: 1.0.0\n",
            )
            .write(
                &format!("{parent}/charts/web/contracts/app.json"),
                &vendored("1"),
            )
            .write(
                &format!("{parent}/charts/web/contracts/gone.json"),
                &vendored("1"),
            );
        repository.commit("nested tree");
        repository.git(&["checkout", "--quiet", "-B", "change", "main"]);
        repository.git(&[
            "rm",
            "--quiet",
            &format!("{parent}/charts/web/contracts/gone.json"),
        ]);
        repository.write(
            &format!("{parent}/charts/web/contracts/app.json"),
            &vendored("2"),
        );
        repository.commit("change");

        let output = isolated(Command::new(env!("CARGO_BIN_EXE_terrace-contract")))
            .current_dir(repository.path())
            .env(
                "TERRACE_CONFIG",
                repository.path().join(format!("{parent}/terrace.yaml")),
            )
            .args(["diff", "--since", "main", "--exit-code"])
            .output()
            .expect("the binary runs");
        let shown = text(&output);
        assert_eq!(code(&output), 2, "{parent}: {shown}");
        // Read at the base, not reported as added: the chart's version there is known, the
        // contract is a change rather than a new file, and the one deleted since is found only by
        // listing the base.
        assert!(shown.contains("1.0.0 at main"), "{parent}: {shown}");
        assert!(shown.contains("(changed)"), "{parent}: {shown}");
        assert!(
            shown.contains("contracts/gone.json  (removed)"),
            "{parent}: {shown}"
        );
    }
}

#[test]
fn shapes_check_over_charts_without_a_shape_says_it_checked_nothing() {
    let repository = Repository::new();
    let output = repository.run(&["shapes", "--check"]);
    let shown = text(&output);
    assert_eq!(code(&output), 0, "{shown}");
    assert!(shown.contains("nothing to check"), "{shown}");
    assert!(!shown.contains("all current"), "{shown}");
}

#[test]
fn check_with_json_output_prints_only_the_report_when_no_chart_is_contracted() {
    let repository = Repository::new();
    std::fs::create_dir_all(repository.path().join("rendered")).expect("render dir");
    let output = repository.run(&["check", "--format", "json"]);
    assert_eq!(code(&output), 0, "{}", text(&output));
    let report: Json =
        serde_json::from_slice(&output.stdout).expect("stdout holds one JSON document");
    assert!(report["findings"].is_array(), "{report}");
}
