//! `migrate`, through the binary, against a real Git repository: the removals a contract refresh
//! cannot follow on its own, and the exit code a caller tells them apart by.
//!
//! The fixture is the shape `tankovault` 11 moved away from: a key of its own (`legal.dir`), and a
//! closed map element that lost a property (`legal.documents.*.sources`). Both are bound at the
//! committed revision and the values blocks are gone from the working tree, which is the state a
//! bump branch is in by the time anyone looks — so every finding here also proves the markers were
//! read at the revision rather than from the working tree.

#![cfg(feature = "cli")]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value as Json, json};

/// What a test process must not inherit: Git's repository-locating variables, and the variables
/// that would make the binary read the caller's configuration.
const INHERITED: [&str; 10] = [
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
];

const VALUES_BEFORE: &str = "\
# @schema
# additionalProperties: true
# @schema
# -- Legal documents.
legal:
  # @schema
  # # @config projection legal.dir optional
  # type: [string, 'null']
  # @schema
  # -- Root the sources resolve against.
  dir: /etc/legal

  # @schema
  # # @config structured legal.documents optional
  # type: [object, 'null']
  # @schema
  # -- The published documents.
  documents: null
";

const TEMPLATE_STALE: &str = "\
{{- define \"fixture.legal\" -}}
{{- range $slug, $doc := .Values.legal.documents -}}
{{- range $locale, $path := ($doc.sources | default dict) -}}{{- end -}}
{{- end -}}
dir: {{ .Values.legal.dir | quote }}
{{- if not $tls.ca.bundle.sources -}}{{- end -}}
{{- end -}}
";

const TEMPLATE_MIGRATED: &str = "\
{{- define \"fixture.legal\" -}}
{{- range $slug, $doc := .Values.legal.documents -}}
{{- range $locale, $body := ($doc.body | default dict) -}}{{- end -}}
{{- end -}}
{{- if not $tls.ca.bundle.sources -}}{{- end -}}
{{- end -}}
";

/// Unrelated to `legal`, and spelling `sources:` as a projected volume does. Never reported.
const TEMPLATE_UNRELATED: &str = "\
- name: secrets
  projected:
    sources:
      - secret: {}
";

const SUITE_STALE: &str = "\
tests:
  - it: passes a path through
    set:
      legal.documents.privacy.sources.en: /mnt/privacy.md
";

const CI_STALE: &str = "\
legal:
  documents:
    privacy:
      sources:
        en: /mnt/privacy.md
";

/// A CI values file with a `sources:` key under some other top-level block. Never reported.
const CI_UNRELATED: &str = "\
networkPolicy:
  sources:
    - a
";

/// One map element's schema, with the properties it declares.
fn element(properties: &Json) -> Json {
    json!({
        "type": "object",
        "additionalProperties": {
            "type": "object",
            "additionalProperties": false,
            "properties": properties,
        },
    })
}

/// One vendored contract with the keys given, pinned at `digest`.
fn vendored(digest: &str, keys: &Json) -> String {
    serde_json::to_string_pretty(&json!({
        "source": {
            "image": "ghcr.io/x/fixture",
            "digest": digest,
            "sha256": "ab",
            "fetched": "2026-01-01T00:00:00Z",
        },
        "contract": {
            "terrace_contract": 1,
            "producer": {"name": "x", "version": "1", "loader": "figment"},
            "app": {"name": "fixture", "version": "1.0.0"},
            "schema": {
                "schema_version": 2,
                "dialect": {"prefix": "P_", "nesting_separator": "__", "indirection_suffix": "_FILE"},
                "loader": [],
                "keys": keys,
            },
            "json_schema": {},
            "external": {"env": [], "ignore": [], "unknown": "reject"},
        },
    }))
    .expect("a fixture serialises")
}

fn key(path: &str, text_form: &str, constraint: Option<Json>) -> Json {
    let mut held = json!({
        "path": path,
        "env": format!("P_{}", path.to_uppercase().replace('.', "__")),
        "text_form": text_form,
        "required": false,
    });
    if let Some(constraint) = constraint {
        held["constraint"] = constraint;
    }
    held
}

/// The contract before the bump: `legal.dir`, and documents whose element carries `sources`.
fn contract_before() -> String {
    vendored(
        "sha256:a",
        &json!([
            key("legal.dir", "text", None),
            key(
                "legal.documents",
                "structured",
                Some(element(
                    &json!({"sources": {"type": "object"}, "url": {"type": "string"}}),
                )),
            ),
        ]),
    )
}

/// The contract after it: `legal.dir` gone, `sources` replaced by `body`, and a key added.
fn contract_after() -> String {
    vendored(
        "sha256:b",
        &json!([
            key("legal.default_locale", "text", None),
            key(
                "legal.documents",
                "structured",
                Some(element(
                    &json!({"body": {"type": "object"}, "url": {"type": "string"}}),
                )),
            ),
        ]),
    )
}

/// A repository whose committed revision is the chart before the bump, and whose working tree has
/// the refreshed contract and the value blocks already removed. Deleted when dropped.
struct Repository(PathBuf);

impl Repository {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "terrace-migrate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let repository = Self(root);
        repository.git(&["init", "--quiet", "--initial-branch=main"]);
        repository
            .write(
                "charts/fixture/Chart.yaml",
                "apiVersion: v2\nname: fixture\nversion: 1.0.0\n",
            )
            .write("charts/fixture/values.yaml", VALUES_BEFORE)
            .write("charts/fixture/contracts/api.json", &contract_before());
        repository.git(&["add", "--all"]);
        repository.git(&["commit", "--quiet", "-m", "before"]);
        // The value blocks are gone by the time anyone looks; the markers must come from the
        // revision.
        repository
            .write("charts/fixture/values.yaml", "legal: {}\n")
            .write("charts/fixture/contracts/api.json", &contract_after());
        repository
    }

    fn git(&self, args: &[&str]) {
        let mut command = Command::new("git");
        for name in INHERITED {
            command.env_remove(name);
        }
        let output = command
            .current_dir(&self.0)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?}: {}", text(&output));
    }

    fn write(&self, relative: &str, body: &str) -> &Self {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(path, body).expect("write");
        self
    }

    /// Write one file of the chart.
    fn chart_file(&self, relative: &str, body: &str) -> &Self {
        self.write(&format!("charts/fixture/{relative}"), body)
    }

    /// `migrate` with `extra`, against `main` unless `extra` names a revision.
    fn migrate(&self, extra: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_terrace-contract"));
        for name in INHERITED {
            command.env_remove(name);
        }
        command.current_dir(&self.0).arg("migrate").args(extra);
        if !extra.contains(&"--since") {
            command.args(["--since", "main"]);
        }
        command.output().expect("the binary runs")
    }

    fn found(&self) -> (i32, Json) {
        let output = self.migrate(&["--json"]);
        let parsed = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("migrate --json prints JSON ({e}): {}", text(&output)));
        (code(&output), parsed)
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

/// Each migration's key, mapped to its bound value and its references as (file name, line).
fn by_key(found: &Json) -> std::collections::BTreeMap<String, (Json, Vec<(String, u64)>)> {
    found["migrations"]
        .as_array()
        .expect("a list of migrations")
        .iter()
        .map(|migration| {
            let mut references: Vec<(String, u64)> = migration["references"]
                .as_array()
                .expect("a list of references")
                .iter()
                .map(|reference| {
                    let path = reference["path"].as_str().expect("a path");
                    let name = path.rsplit('/').next().expect("a file name").to_owned();
                    (name, reference["line"].as_u64().expect("a line"))
                })
                .collect();
            references.sort();
            (
                migration["key"].as_str().expect("a key").to_owned(),
                (migration["value"].clone(), references),
            )
        })
        .collect()
}

#[test]
fn every_stale_line_is_named() {
    let repository = Repository::new();
    repository
        .chart_file("templates/_legal.tpl", TEMPLATE_STALE)
        .chart_file("templates/_volumes.tpl", TEMPLATE_UNRELATED)
        .chart_file("tests/legal_test.yaml", SUITE_STALE)
        .chart_file("ci/legal.yaml", CI_STALE)
        .chart_file("ci/netpol.yaml", CI_UNRELATED);

    let (exit, found) = repository.found();
    assert_eq!(exit, 2, "{found}");
    assert_eq!(found["open"], json!(true));
    let found = by_key(&found);
    assert_eq!(
        found.keys().map(String::as_str).collect::<Vec<_>>(),
        ["legal.dir", "legal.documents"]
    );

    let (value, references) = &found["legal.dir"];
    assert_eq!(value, &json!("legal.dir"));
    assert_eq!(references, &[("_legal.tpl".to_owned(), 5)]);

    let (value, references) = &found["legal.documents"];
    assert_eq!(value, &json!("legal.documents"));
    assert_eq!(
        references,
        &[
            ("_legal.tpl".to_owned(), 3),
            ("legal.yaml".to_owned(), 4),
            ("legal_test.yaml".to_owned(), 4),
        ]
    );
}

#[test]
fn the_report_reads_as_it_always_has() {
    // The wording is the one the scripts this replaced printed, so docs and muscle memory hold.
    let repository = Repository::new();
    repository
        .chart_file("templates/_legal.tpl", TEMPLATE_STALE)
        .chart_file("tests/legal_test.yaml", SUITE_STALE)
        .chart_file("ci/legal.yaml", CI_STALE);
    let output = repository.migrate(&[]);
    assert_eq!(code(&output), 2, "{}", text(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "\
==> hand migration required: the contract took these away, and no generator edits
    the templates, hand-written tests or CI values that still use them
  fixture: legal.dir: key removed (chart value legal.dir)
      charts/fixture/templates/_legal.tpl:5: dir: {{ .Values.legal.dir | quote }}
  fixture: legal.documents: element property 'sources' removed (chart value legal.documents)
      charts/fixture/templates/_legal.tpl:3: {{- range $locale, $path := ($doc.sources | default dict) -}}{{- end -}}
      charts/fixture/tests/legal_test.yaml:4: legal.documents.privacy.sources.en: /mnt/privacy.md
      charts/fixture/ci/legal.yaml:4: sources:
"
    );
}

#[test]
fn a_migrated_chart_passes() {
    let repository = Repository::new();
    repository.chart_file("templates/_legal.tpl", TEMPLATE_MIGRATED);
    let (exit, found) = repository.found();
    assert_eq!(exit, 0, "{found}");
    assert_eq!(found["open"], json!(false));
    let found = by_key(&found);
    assert_eq!(found.len(), 2);
    assert!(found.values().all(|(_, references)| references.is_empty()));

    let output = repository.migrate(&[]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "\
==> removed by the contract, and no longer referenced by the chart:
  fixture: legal.dir: key removed (chart value legal.dir)
  fixture: legal.documents: element property 'sources' removed (chart value legal.documents)
"
    );
}

#[test]
fn the_generated_round_trip_suites_are_skipped() {
    let repository = Repository::new();
    repository.chart_file("tests/contract_roundtrip_api_test.yaml", SUITE_STALE);
    let (exit, found) = repository.found();
    assert_eq!(exit, 0, "{found}");
    assert!(
        by_key(&found)
            .values()
            .all(|(_, references)| references.is_empty())
    );
}

#[test]
fn a_removal_no_chart_value_was_bound_to_has_nothing_to_search_for() {
    let repository = Repository::new();
    // A second commit whose values carry no marker, with the contract still the one before.
    repository.write("charts/fixture/contracts/api.json", &contract_before());
    repository.git(&["commit", "--quiet", "--all", "-m", "unbound"]);
    repository
        .write("charts/fixture/contracts/api.json", &contract_after())
        .chart_file("templates/_legal.tpl", TEMPLATE_STALE);

    // Against it, neither removal reached a chart value, so there is nothing to search for —
    // whatever the templates still say.
    let output = repository.migrate(&[]);
    assert_eq!(code(&output), 0, "{}", text(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "\
==> removed by the contract, and no longer referenced by the chart:
  fixture: legal.dir: key removed (no chart value was bound to it)
  fixture: legal.documents: element property 'sources' removed (no chart value was bound to it)
"
    );

    // Against the first commit, whose values bound both, the same working tree is stale: the
    // markers are the revision's, not the working tree's.
    let output = repository.migrate(&["--since", "main~1"]);
    assert_eq!(code(&output), 2, "{}", text(&output));
}

#[test]
fn nothing_removed_is_said_once() {
    let repository = Repository::new();
    repository.write("charts/fixture/contracts/api.json", &contract_before());
    let output = repository.migrate(&[]);
    assert_eq!(code(&output), 0, "{}", text(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "==> no contract change removes anything a chart uses\n"
    );
}

#[test]
fn the_exit_status_follows_the_references() {
    let repository = Repository::new();

    repository.chart_file("templates/_legal.tpl", TEMPLATE_MIGRATED);
    assert_eq!(code(&repository.migrate(&[])), 0);

    repository.chart_file("templates/_legal.tpl", TEMPLATE_STALE);
    assert_eq!(code(&repository.migrate(&[])), 2);

    // A contract that cannot be read is a failure to answer, never "nothing to migrate" and never
    // "something to migrate" — and no finding is printed past it, because the unreadable side
    // would otherwise read as every key removed.
    repository.write("charts/fixture/contracts/api.json", "{");
    let output = repository.migrate(&[]);
    assert_eq!(code(&output), 1, "{}", text(&output));
    assert!(output.stdout.is_empty(), "{}", text(&output));
}

#[test]
fn a_question_it_cannot_answer_exits_one() {
    let repository = Repository::new();
    repository.chart_file("templates/_legal.tpl", TEMPLATE_STALE);

    let output = repository.migrate(&["--since", "no-such-revision"]);
    assert_eq!(code(&output), 1, "{}", text(&output));

    // A misspelt chart is refused rather than comparing nothing and passing.
    let output = repository.migrate(&["fixtrue"]);
    assert_eq!(code(&output), 1, "{}", text(&output));

    // The chart named correctly answers as the whole tree does.
    assert_eq!(code(&repository.migrate(&["fixture"])), 2);
}

#[test]
fn markers_that_cannot_be_read_at_the_revision_are_a_failure_to_answer() {
    let repository = Repository::new();
    repository.write(
        "charts/fixture/values.yaml",
        "legal:\n  # @schema\n  # # @config nonsense legal.dir\n  # @schema\n  dir: x\n",
    );
    repository.write("charts/fixture/contracts/api.json", &contract_before());
    repository.git(&["commit", "--quiet", "--all", "-m", "broken markers"]);
    repository.write("charts/fixture/contracts/api.json", &contract_after());
    let output = repository.migrate(&[]);
    assert_eq!(code(&output), 1, "{}", text(&output));
}
