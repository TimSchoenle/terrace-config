//! `helm init`, through the binary, in its plain form: which settings decide whether a declaration
//! is owed, which mode writes, and which exit code a pipeline sees.
//!
//! The contracted form needs a registry and `cosign`, and is unit-tested in `src/helm/init.rs`
//! against a registry double that answers exactly what the verifying fetch asks. What only a process
//! can show is the part the double cannot: that `--check` and `--diff` leave the tree as it was, and
//! which exit code each refusal carries.

#![cfg(feature = "cli")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A Git repository root in the temporary directory, removed when dropped.
struct Repository(PathBuf);

impl Repository {
    fn new(terrace: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "terrace-init-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).expect("git dir");
        std::fs::create_dir_all(root.join("charts")).expect("charts dir");
        std::fs::write(root.join("terrace.yaml"), terrace).expect("terrace.yaml");
        Self(root)
    }

    fn write(&self, relative: &str, text: &str) -> &Self {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(path, text).expect("write");
        self
    }

    fn read(&self, relative: &str) -> Option<String> {
        std::fs::read_to_string(self.0.join(relative)).ok()
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }

    /// Run `helm init` from the root, with no setting inherited from the environment.
    fn init(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_terrace-contract"));
        command
            .current_dir(&self.0)
            .args(["helm", "init"])
            .args(args);
        for name in [
            "TERRACE_CONFIG",
            "TERRACE_STRICT",
            "TERRACE_CHARTS",
            "CONTRACT_SIGNER",
        ] {
            command.env_remove(name);
        }
        command.output().expect("the binary runs")
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(output: &Output) -> String {
    format!(
        "status {:?}\n--- stdout\n{}--- stderr\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

const FIRST_PARTY: &str = "version: 1\nhelm:\n  firstParty: [org/*]\n";

const REDIS: [&str; 4] = ["redis", "--image", "library/redis:8.0", "--no-contract"];

fn exists(path: &Path) -> bool {
    path.exists()
}

#[test]
fn the_default_prints_the_plan_and_writes_nothing() {
    let repository = Repository::new(FIRST_PARTY);
    let output = repository.init(&REDIS);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "    create     {}\n",
            repository.path("charts/redis/values.yaml").display()
        )),
        "{stdout}"
    );
    assert!(!exists(&repository.path("charts/redis/values.yaml")));
}

#[test]
fn check_fails_while_anything_is_owed_and_passes_once_written() {
    let repository = Repository::new(FIRST_PARTY);
    repository.write(
        "charts/redis/templates/deployment.yaml",
        "prefix: %%ENV_PREFIX%%\n",
    );

    let owed = repository.init(&[&REDIS[..], &["--check"]].concat());
    assert_eq!(owed.status.code(), Some(1), "{}", text(&owed));
    assert!(!exists(&repository.path("charts/redis/values.yaml")));

    let written = repository.init(&[&REDIS[..], &["--write"]].concat());
    assert_eq!(written.status.code(), Some(0), "{}", text(&written));
    assert!(
        String::from_utf8_lossy(&written.stdout).contains("configuration written"),
        "{}",
        text(&written)
    );
    assert_eq!(
        repository
            .read("charts/redis/templates/deployment.yaml")
            .as_deref(),
        Some("prefix: REDIS\n")
    );
    assert!(
        repository
            .read("charts/redis/ci/test-values.yaml")
            .is_some()
    );
    // A third-party image owes no declaration.
    assert!(!exists(
        &repository.path("charts/redis/config-contract.yaml")
    ));

    let clean = repository.init(&[&REDIS[..], &["--check"]].concat());
    assert_eq!(clean.status.code(), Some(0), "{}", text(&clean));
}

#[test]
fn diff_prints_the_edit_and_writes_nothing() {
    let repository = Repository::new(FIRST_PARTY);
    // Relative to where it runs, so the output applies with `patch -p0` from the root.
    let output = repository.init(&[&REDIS[..], &["--charts", "charts", "--diff"]].concat());
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--- /dev/null\n+++ charts/redis/values.yaml\n"),
        "{stdout}"
    );
    assert!(stdout.contains("+  tag: \"8.0\"\n"), "{stdout}");
    assert!(!exists(&repository.path("charts/redis")));
}

#[test]
fn a_configured_chart_is_refused() {
    let repository = Repository::new(FIRST_PARTY);
    repository.write("charts/redis/values.yaml", "image: {}\n");
    let output = repository.init(&[&REDIS[..], &["--write"]].concat());
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("charts/redis/values.yaml"),
        "{}",
        text(&output)
    );
    assert_eq!(
        repository.read("charts/redis/values.yaml").as_deref(),
        Some("image: {}\n")
    );

    // A check that cannot answer says so with the code that means "could not read".
    let checked = repository.init(&[&REDIS[..], &["--check"]].concat());
    assert_eq!(checked.status.code(), Some(2), "{}", text(&checked));
}

#[test]
fn a_first_party_image_with_no_contract_owes_a_reason() {
    let repository = Repository::new(FIRST_PARTY);
    let bare = ["app", "--image", "org/app:v1", "--no-contract", "--write"];
    let refused = repository.init(&bare);
    assert_eq!(refused.status.code(), Some(1), "{}", text(&refused));
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("--reason"),
        "{}",
        text(&refused)
    );
    assert!(!exists(&repository.path("charts/app")));

    let given = repository.init(&[&bare[..], &["--reason", "Configured by flags alone."]].concat());
    assert_eq!(given.status.code(), Some(0), "{}", text(&given));
    let declaration = repository
        .read("charts/app/config-contract.yaml")
        .expect("the opt-out is written");
    assert!(declaration.contains("documents: []"), "{declaration}");
    assert!(
        declaration.contains("Configured by flags alone."),
        "{declaration}"
    );
}

#[test]
fn no_resolved_first_party_list_is_refused_rather_than_guessed() {
    let repository = Repository::new("version: 1\n");
    let output = repository.init(&REDIS);
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("helm.firstParty"),
        "{}",
        text(&output)
    );
}

#[test]
fn a_reason_without_no_contract_is_a_usage_error() {
    let repository = Repository::new(FIRST_PARTY);
    let output = repository.init(&["app", "--image", "org/app:v1", "--reason", "x"]);
    assert_eq!(output.status.code(), Some(2), "{}", text(&output));
}

#[test]
fn check_and_write_are_exclusive() {
    let repository = Repository::new(FIRST_PARTY);
    let output = repository.init(&[&REDIS[..], &["--check", "--write"]].concat());
    assert_eq!(output.status.code(), Some(2), "{}", text(&output));
}
