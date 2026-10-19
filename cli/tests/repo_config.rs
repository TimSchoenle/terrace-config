//! `terrace.yaml`, through the binary: discovery, precedence, the unknown-key policy and the
//! `config` and `doctor` commands. The parsing rules themselves are unit-tested in `src/repo.rs`;
//! what only a process can show is which layer wins and which exit code a pipeline sees.

#![cfg(feature = "cli")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value as Json;

/// A Git repository root in the temporary directory, removed when dropped.
struct Repository(PathBuf);

impl Repository {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "terrace-repo-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).expect("git dir");
        Self(root)
    }

    fn write(&self, relative: &str, text: &str) -> &Self {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(path, text).expect("write");
        self
    }

    fn chart(&self, name: &str, repository: &str) -> &Self {
        self.write(
            &format!("charts/{name}/Chart.yaml"),
            &format!("name: {name}\n"),
        );
        self.write(
            &format!("charts/{name}/values.yaml"),
            &format!("image:\n  repository: {repository}\n  tag: \"1\"\n"),
        )
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }

    /// Run the binary in `directory` (relative to the root), with no setting inherited from the
    /// environment the tests run in.
    fn run(&self, directory: &str, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_terrace-contract"));
        command.current_dir(self.0.join(directory)).args(args);
        for name in [
            "TERRACE_CONFIG",
            "TERRACE_STRICT",
            "TERRACE_CHARTS",
            "CONTRACT_SIGNER",
        ] {
            command.env_remove(name);
        }
        command.envs(env.iter().copied());
        command.output().expect("the binary runs")
    }

    fn show(&self, directory: &str, env: &[(&str, &str)]) -> Json {
        let output = self.run(directory, &["config", "show", "--json"], env);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).expect("config show --json prints JSON")
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

fn shown_path(value: &Json) -> PathBuf {
    PathBuf::from(value.as_str().expect("a path"))
}

#[test]
fn without_a_file_every_setting_is_a_default_relative_to_the_working_directory() {
    let repository = Repository::new();
    let shown = repository.show("", &[]);
    assert_eq!(shown["file"], Json::Null);
    assert_eq!(shown["charts"]["value"], "charts");
    assert_eq!(shown["charts"]["source"], "default");
    assert_eq!(shown["signer"], Json::Null);
}

#[test]
fn the_file_at_the_git_root_is_found_from_a_subdirectory_and_resolves_against_itself() {
    let repository = Repository::new();
    repository
        .write(
            "terrace.yaml",
            "version: 1\nhelm:\n  charts: deploy\n  signer: me\n",
        )
        .write("deploy/a/Chart.yaml", "name: a\n");
    let shown = repository.show("deploy/a", &[]);
    assert_eq!(shown_path(&shown["file"]), repository.path("terrace.yaml"));
    assert_eq!(
        shown_path(&shown["charts"]["value"]),
        repository.path("deploy")
    );
    assert_eq!(shown["charts"]["source"], "terrace.yaml");
    assert_eq!(shown["signer"]["source"], "terrace.yaml");
}

#[test]
fn a_built_in_default_resolves_against_the_file_once_there_is_one() {
    let repository = Repository::new();
    repository
        .write("terrace.yaml", "version: 1\n")
        .write("sub/.keep", "");
    let shown = repository.show("sub", &[]);
    assert_eq!(
        shown_path(&shown["charts"]["value"]),
        repository.path("charts")
    );
    assert_eq!(shown["charts"]["source"], "default");
}

#[test]
fn the_environment_wins_over_the_file() {
    let repository = Repository::new();
    repository.write(
        "terrace.yaml",
        "version: 1\nhelm:\n  charts: deploy\n  signer: file\n",
    );
    let shown = repository.show(
        "",
        &[("TERRACE_CHARTS", "elsewhere"), ("CONTRACT_SIGNER", "env")],
    );
    assert_eq!(shown["charts"]["value"], "elsewhere");
    assert_eq!(shown["charts"]["source"], "environment");
    assert_eq!(shown["signer"]["value"], "env");
    assert_eq!(shown["signer"]["source"], "environment");
}

#[test]
fn a_flag_wins_over_the_file() {
    let repository = Repository::new();
    repository
        .write(
            "terrace.yaml",
            "version: 1\nhelm:\n  charts: missing\n  firstParty: [ghcr.io/x/*]\n",
        )
        .chart("a", "ghcr.io/x/a");
    let from_file = repository.run("", &["coverage"], &[]);
    assert_eq!(
        code(&from_file),
        2,
        "the file's tree does not exist: {}",
        text(&from_file)
    );
    let from_flag = repository.run("", &["coverage", "--charts", "charts"], &[]);
    assert_eq!(code(&from_flag), 0, "{}", text(&from_flag));
    assert!(
        text(&from_flag).contains("a: pins the first-party image"),
        "{}",
        text(&from_flag)
    );
}

#[test]
fn coverage_reads_the_first_party_patterns_from_the_file() {
    let repository = Repository::new();
    repository
        .write(
            "terrace.yaml",
            "version: 1\nhelm:\n  firstParty: [ghcr.io/x/*]\n",
        )
        .chart("ours", "ghcr.io/x/ours")
        .chart("theirs", "docker.io/library/theirs");
    let output = repository.run("", &["coverage", "--format", "json"], &[]);
    assert_eq!(code(&output), 0, "{}", text(&output));
    let found = text(&output);
    assert!(found.contains("ghcr.io/x/ours"), "{found}");
    assert!(!found.contains("docker.io/library/theirs"), "{found}");
}

#[test]
fn an_unknown_key_warns_and_fails_only_under_strict() {
    let repository = Repository::new();
    repository
        .write(
            "terrace.yaml",
            "version: 1\ncli: \"99.0\"\nlater: 1\nx-team: a\nhelm:\n  firstParty: []\n",
        )
        .chart("a", "ghcr.io/x/a");

    let lenient = repository.run("", &["coverage"], &[]);
    assert_eq!(code(&lenient), 0, "{}", text(&lenient));
    let warned = String::from_utf8_lossy(&lenient.stderr);
    assert!(warned.contains("`later`"), "{warned}");
    assert!(
        warned.contains("pins cli 99.0"),
        "the newer pin is named: {warned}"
    );
    assert!(
        !warned.contains("x-team"),
        "extensions are never reported: {warned}"
    );

    for strict in [
        repository.run("", &["coverage", "--strict"], &[]),
        repository.run("", &["coverage"], &[("TERRACE_STRICT", "1")]),
        repository.run("", &["coverage"], &[("TERRACE_STRICT", "true")]),
    ] {
        assert_eq!(code(&strict), 2, "{}", text(&strict));
    }
    let off = repository.run("", &["coverage"], &[("TERRACE_STRICT", "0")]);
    assert_eq!(code(&off), 0, "{}", text(&off));
}

#[test]
fn an_unsupported_version_and_a_missing_named_file_are_errors() {
    let repository = Repository::new();
    repository
        .write("terrace.yaml", "version: 2\n")
        .chart("a", "ghcr.io/x/a");
    let later = repository.run("", &["readme", "--check"], &[]);
    assert_eq!(code(&later), 2, "{}", text(&later));
    assert!(text(&later).contains("`version: 2`"), "{}", text(&later));

    let named = repository.run("", &["config", "show", "--config", "nope.yaml"], &[]);
    assert_eq!(code(&named), 2, "{}", text(&named));
    assert!(text(&named).contains("nope.yaml"), "{}", text(&named));
}

#[test]
fn a_document_command_does_not_read_the_file() {
    let repository = Repository::new();
    repository.write("terrace.yaml", "version: 2\n");
    let output = repository.run("", &["validate", "missing.json"], &[]);
    let said = text(&output);
    assert!(
        said.contains("missing.json"),
        "fails on its own input: {said}"
    );
    assert!(
        !said.contains("terrace.yaml"),
        "and not on the repository file: {said}"
    );
}

#[test]
fn pull_without_a_signer_from_any_layer_is_refused_before_touching_a_registry() {
    let repository = Repository::new();
    repository.write("terrace.yaml", "version: 1\n");
    let output = repository.run("", &["pull"], &[]);
    assert_eq!(code(&output), 2, "{}", text(&output));
    assert!(text(&output).contains("helm.signer"), "{}", text(&output));
}

#[test]
fn init_writes_a_file_that_reads_back_and_carries_the_first_party_list_over() {
    let repository = Repository::new();
    repository
        .write(
            ".github/configs/first-party-images.txt",
            "# ours\nghcr.io/x/*\n\nghcr.io/y/app\n",
        )
        .write("charts/.keep", "");

    let first = repository.run("charts", &["config", "init"], &[]);
    assert_eq!(code(&first), 0, "{}", text(&first));
    assert!(repository.path("terrace.yaml").is_file());

    let shown = repository.show("", &[]);
    assert_eq!(
        shown["firstParty"]["value"],
        serde_json::json!(["ghcr.io/x/*", "ghcr.io/y/app"])
    );
    assert_eq!(shown["firstParty"]["source"], "terrace.yaml");
    assert_eq!(shown["unknown"], serde_json::json!([]));
    let pin = shown["cli"].as_str().expect("a pin");
    let running = shown["running"].as_str().expect("the running release");
    assert!(
        running.starts_with(&format!("{pin}.")),
        "{pin} pins {running}'s minor"
    );

    let again = repository.run("", &["config", "init"], &[]);
    assert_eq!(
        code(&again),
        2,
        "an existing file is not replaced: {}",
        text(&again)
    );
    let forced = repository.run("", &["config", "init", "--force"], &[]);
    assert_eq!(code(&forced), 0, "{}", text(&forced));
}

#[test]
fn doctor_fails_on_what_would_break_a_gate_and_warns_on_the_rest() {
    let repository = Repository::new();
    repository.chart("a", "ghcr.io/x/a");
    repository.write("terrace.yaml", "version: 1\nhelm:\n  firstParty: []\n");
    let healthy = repository.run(
        "",
        &["doctor"],
        &[("ORAS_BIN", "oras"), ("COSIGN_BIN", "cosign")],
    );
    assert_eq!(code(&healthy), 0, "{}", text(&healthy));
    assert!(
        text(&healthy).contains("no `cli` pin"),
        "{}",
        text(&healthy)
    );

    repository.write(
        "terrace.yaml",
        "version: 1\ncli: \"0.0\"\nlater: 1\nhelm:\n  charts: gone\n",
    );
    let broken = repository.run("", &["doctor"], &[]);
    assert_eq!(code(&broken), 1, "{}", text(&broken));
    let said = String::from_utf8_lossy(&broken.stdout);
    for expected in ["fail  `later`", "fail  cli 0.0 pinned", "fail  charts:"] {
        assert!(said.contains(expected), "{expected}: {said}");
    }
}

#[test]
fn a_named_file_replaces_discovery() {
    let repository = Repository::new();
    repository
        .write("terrace.yaml", "version: 1\nhelm:\n  signer: discovered\n")
        .write("other/terrace.yaml", "version: 1\nhelm:\n  signer: named\n");
    let by_flag = repository.show("", &[]);
    assert_eq!(by_flag["signer"]["value"], "discovered");
    let named = repository.path("other/terrace.yaml");
    let by_env = repository.show("", &[("TERRACE_CONFIG", named.to_str().expect("utf-8"))]);
    assert_eq!(by_env["signer"]["value"], "named");
    assert!(Path::new(by_env["file"].as_str().expect("a path")).ends_with("other/terrace.yaml"));
}

#[test]
fn doctor_reports_an_unreadable_file_and_carries_on() {
    let repository = Repository::new();
    repository
        .chart("a", "ghcr.io/x/a")
        .write("terrace.yaml", "version: 1\nhelm: [1]\n");
    let output = repository.run("", &["doctor"], &[]);
    assert_eq!(code(&output), 1, "{}", text(&output));
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(said.contains("fail  ") && said.contains("/helm"), "{said}");
    assert!(
        said.contains("ok    charts:"),
        "the defaults are still checked: {said}"
    );
}
