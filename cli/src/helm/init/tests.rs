//! The planner, against a chart directory on disk and a registry double.
//!
//! What the renderers write is [`super::super::scaffold::chart`]'s suite. These are about the part
//! only a run against a directory shows: which files are created and which are filled, that a
//! second run owes nothing, that nothing is ever overwritten, that a placeholder nobody substituted
//! is refused, and that the contract reaches the chart only through the fetch that verifies it: the
//! double answers exactly what [`fetch_pinned`] asks, so a contract that skipped a proof would not
//! be served.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value as Json, json};

use super::{Action, Init, Pin, Request, Source, owes_a_declaration, plan};
use crate::document::{LABEL_PREFIX, LABEL_VERSION};
use crate::error::Error;
use crate::helm::pull::{Fetched, fetch_pinned};
use crate::oci::{ARTIFACT_TYPE, Registry, hex_sha256};

const IMAGE: &str = "org/app:v1.0.0@sha256:0123";
const ATTACHED: &str = "sha256:attached";

/// A registry serving one contract, attached to the pinned digest, with a valid signature.
struct Serving {
    payload: Vec<u8>,
}

impl Registry for Serving {
    fn resolve(&self, reference: &str) -> Result<String, Error> {
        Err(Error::Invalid(format!(
            "{reference}: the tests pin a digest"
        )))
    }

    fn discover(&self, reference: &str, _: &str) -> Result<Vec<Json>, Error> {
        Ok(if reference.ends_with(ATTACHED) {
            Vec::new()
        } else {
            vec![json!({"digest": ATTACHED, "artifactType": ARTIFACT_TYPE})]
        })
    }

    fn image_labels(&self, _: &str) -> Result<BTreeMap<String, String>, Error> {
        Ok([
            (LABEL_VERSION.to_owned(), "1".to_owned()),
            (LABEL_PREFIX.to_owned(), "APP_".to_owned()),
        ]
        .into_iter()
        .collect())
    }

    fn manifest(&self, reference: &str) -> Result<Json, Error> {
        Ok(if reference.ends_with(ATTACHED) {
            json!({"layers": [{"digest": format!("sha256:{}", hex_sha256(&self.payload))}]})
        } else {
            json!({"layers": []})
        })
    }

    fn blob(&self, _: &str) -> Result<Vec<u8>, Error> {
        Ok(self.payload.clone())
    }

    fn verify(&self, _: &str, _: &str) -> Result<(), Error> {
        Ok(())
    }
}

fn key(path: &str, overrides: &Json) -> Json {
    let spelt = path.replace('.', "__");
    let mut fields = json!({
        "path": path,
        "env": format!("APP_{}", spelt.to_uppercase()),
        "env_file": format!("APP_{}_FILE", spelt.to_uppercase()),
        "secrets_file": spelt,
        "docs": format!("Documentation for {path}."),
        "constraint": {"type": "string"},
        "text_form": "text",
        "default": null,
        "default_value": null,
        "required": false,
        "secret": false,
        "reserved": false,
    });
    for (name, value) in overrides.as_object().expect("an object") {
        fields[name] = value.clone();
    }
    fields
}

/// The contract every contracted case fetches: a setting, a required credential and a required
/// key with no published default, under one grouping block.
fn fetched() -> Fetched {
    let contract = json!({
        "terrace_contract": 1,
        "app": {"name": "app", "version": "1.0.0"},
        "schema": {
            "schema_version": 2,
            "dialect": {
                "prefix": "APP_",
                "nesting_separator": "__",
                "indirection_suffix": "_FILE",
            },
            "loader": [],
            "keys": [
                key("a.b", &json!({})),
                key("a.port", &json!({"required": true, "text_form": "integer",
                                      "constraint": {"type": "integer", "minimum": 1}})),
                key("a.token", &json!({"secret": true, "required": true})),
            ],
        },
        "json_schema": {},
        "external": {"env": [], "ignore": [], "unknown": "reject"},
    });
    let registry = Serving {
        payload: serde_json::to_vec(&contract).expect("it serialises"),
    };
    let pin = Pin::parse(IMAGE).expect("a pin");
    fetch_pinned(&registry, &pin.block(), None, "signer").expect("the double serves it")
}

/// A chart tree in the temporary directory, removed when dropped.
struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "terrace-init-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a chart tree");
        Self(root)
    }

    /// One file under `charts/app/`, as a repository's template copy would leave it.
    fn copied(&self, relative: &str, text: &str) -> &Self {
        let path = self.chart().join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(path, text).expect("a file");
        self
    }

    fn chart(&self) -> PathBuf {
        self.0.join("app")
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.chart().join(relative)).expect("the file exists")
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const DEPLOYMENT: &str = "\
env:
  - name: %%PREFIX%%CONFIG
    value: {{ .Values.configMount.configDir }}/%%DOCUMENT_KEY%%
{{- include \"common.fileConfig.env\" (dict \"prefix\" \"%%ENV_PREFIX%%\") }}
";

fn run(tree: &Tree, source: &Source<'_>) -> Result<Init, Error> {
    let pin = Pin::parse(IMAGE).expect("a pin");
    plan(
        &tree.0,
        &Request {
            chart: "app",
            pin: &pin,
            document: "app",
            document_key: "config.toml",
            chassis: "",
        },
        source,
    )
}

/// Every change, as `(path under the chart, action)`.
fn actions(init: &Init) -> Vec<(String, Action)> {
    init.changes
        .iter()
        .map(|change| {
            let relative = change
                .path
                .strip_prefix(&init.chart_dir)
                .expect("under the chart");
            (
                relative.to_string_lossy().replace('\\', "/"),
                change.action.clone(),
            )
        })
        .collect()
}

fn created(paths: &[&str]) -> Vec<(String, Action)> {
    paths
        .iter()
        .map(|path| ((*path).to_owned(), Action::Create))
        .collect()
}

// --------------------------------------------------------------------------------------
// The pin and the first-party question
// --------------------------------------------------------------------------------------

#[test]
fn a_pin_is_a_repository_and_a_tag_with_an_optional_digest() {
    let parse = |reference: &str| {
        let pin = Pin::parse(reference).expect("a pin");
        (pin.repository, pin.tag)
    };
    assert_eq!(parse("org/app:v1"), ("org/app".into(), "v1".into()));
    assert_eq!(
        parse("org/app:v1@sha256:ab"),
        ("org/app".into(), "v1@sha256:ab".into())
    );
    // A registry with a port: the colon before the last slash is not the tag's.
    assert_eq!(
        parse("registry.local:5000/org/app:8.0"),
        ("registry.local:5000/org/app".into(), "8.0".into())
    );
}

#[test]
fn a_pin_without_a_tag_is_refused() {
    for reference in [
        "org/app",
        "org/app@sha256:ab",
        "registry.local:5000/org/app",
        ":v1",
        "org/app:",
        "org/app:v1@",
        "",
    ] {
        assert!(Pin::parse(reference).is_err(), "{reference:?} was accepted");
    }
}

#[test]
fn a_first_party_image_owes_a_declaration() {
    let patterns = ["org/*".to_owned(), "ghcr.io/org/*".to_owned()];
    assert!(owes_a_declaration(&patterns, "org/app"));
    assert!(owes_a_declaration(&patterns, "ghcr.io/org/app"));
    assert!(!owes_a_declaration(&patterns, "library/redis"));
}

#[test]
fn an_empty_list_makes_nothing_first_party() {
    assert!(!owes_a_declaration(&[], "org/app"));
}

// --------------------------------------------------------------------------------------
// The contracted form
// --------------------------------------------------------------------------------------

#[test]
fn every_file_is_planned_and_the_copy_is_filled_from_the_contract() {
    let tree = Tree::new();
    tree.copied("Chart.yaml", "name: app\n")
        .copied("templates/deployment.yaml", DEPLOYMENT);
    let fetched = fetched();
    let init = run(
        &tree,
        &Source::Contract {
            fetched: &fetched,
            at: "2026-01-01T00:00:00Z",
        },
    )
    .expect("it plans");

    let mut expected = created(&[
        "contracts/app.json",
        "values.yaml",
        "templates/_helpers.tpl",
        "config-contract.yaml",
        "ci/test-values.yaml",
        "tests/configmap_test.yaml",
    ]);
    expected.push((
        "templates/deployment.yaml".to_owned(),
        Action::Fill(vec!["DOCUMENT_KEY", "ENV_PREFIX", "PREFIX"]),
    ));
    assert_eq!(actions(&init), expected);
    assert!(init.owed());
    assert_eq!(init.undescribed, ["a"]);
    assert_eq!(init.invented, ["a.port"]);

    // Planning wrote nothing.
    assert!(!tree.chart().join("values.yaml").exists());
    assert_eq!(tree.read("templates/deployment.yaml"), DEPLOYMENT);

    init.write().expect("it writes");
    assert_eq!(
        tree.read("templates/deployment.yaml"),
        DEPLOYMENT
            .replace("%%PREFIX%%", "APP_")
            .replace("%%DOCUMENT_KEY%%", "config.toml")
            .replace("%%ENV_PREFIX%%", "APP")
    );
    let declaration = tree.read("config-contract.yaml");
    assert!(declaration.contains("tests: true"), "{declaration}");
    assert!(
        !declaration.contains("contract:"),
        "the contract path is the default and is left out: {declaration}"
    );
    let vendored: Json =
        serde_json::from_str(&tree.read("contracts/app.json")).expect("vendored JSON");
    assert_eq!(vendored["source"]["image"], json!("docker.io/org/app"));
    assert_eq!(vendored["source"]["digest"], json!("sha256:0123"));
}

#[test]
fn a_second_run_owes_nothing_even_though_the_fetch_time_moved() {
    let tree = Tree::new();
    tree.copied("templates/deployment.yaml", DEPLOYMENT);
    let fetched = fetched();
    run(
        &tree,
        &Source::Contract {
            fetched: &fetched,
            at: "2026-01-01T00:00:00Z",
        },
    )
    .expect("it plans")
    .write()
    .expect("it writes");

    let again = run(
        &tree,
        &Source::Contract {
            fetched: &fetched,
            at: "2026-02-02T00:00:00Z",
        },
    )
    .expect("it plans again");
    assert!(!again.owed(), "{}", again.report());
    assert!(
        again
            .changes
            .iter()
            .all(|change| change.action == Action::Unchanged)
    );
    assert!(
        again.report().contains("nothing to write"),
        "{}",
        again.report()
    );
    assert_eq!(again.diff(), "");
}

#[test]
fn a_file_that_says_something_else_is_refused_and_nothing_is_written() {
    let tree = Tree::new();
    tree.copied("values.yaml", "image: {}\n")
        .copied("config-contract.yaml", "documents: []\n")
        .copied("templates/deployment.yaml", DEPLOYMENT);
    let fetched = fetched();
    let refused = run(
        &tree,
        &Source::Contract {
            fetched: &fetched,
            at: "2026-01-01T00:00:00Z",
        },
    )
    .expect_err("a configured chart is refused")
    .to_string();
    for named in ["app/values.yaml", "app/config-contract.yaml", "adopt"] {
        assert!(refused.contains(named), "{refused}");
    }
    assert_eq!(tree.read("values.yaml"), "image: {}\n");
    assert!(!tree.chart().join("contracts").exists());
}

#[test]
fn a_placeholder_the_copy_left_is_refused() {
    let tree = Tree::new();
    tree.copied("Chart.yaml", "name: %%CHART%%\n");
    let fetched = fetched();
    let refused = run(
        &tree,
        &Source::Contract {
            fetched: &fetched,
            at: "2026-01-01T00:00:00Z",
        },
    )
    .expect_err("a leftover placeholder is refused")
    .to_string();
    assert!(refused.contains("%%CHART%%"), "{refused}");
    assert!(refused.contains("app/Chart.yaml"), "{refused}");
}

#[test]
fn dependencies_and_vendored_contracts_are_not_template_text() {
    // A packaged dependency or a contract's own prose can hold anything; neither is filled.
    let tree = Tree::new();
    tree.copied("charts/common/templates/_x.tpl", "%%PREFIX%% %%OTHER%%\n")
        .copied("contracts/other.json", "{\"docs\": \"%%PREFIX%%\"}\n")
        .copied("docs/%%PREFIX%%.md", "%%PREFIX%%\n");
    let init = run(&tree, &Source::Plain { opt_out: None }).expect("it plans");
    let filled: Vec<String> = actions(&init)
        .into_iter()
        .filter(|(_, action)| matches!(action, Action::Fill(_)))
        .map(|(path, _)| path)
        .collect();
    assert_eq!(filled, ["docs/%%PREFIX%%.md"]);
}

#[test]
fn a_value_that_would_break_a_line_is_refused() {
    let tree = Tree::new();
    tree.copied("templates/configmap.yaml", "key: %%DOCUMENT_KEY%%\n");
    let pin = Pin::parse(IMAGE).expect("a pin");
    let refused = plan(
        &tree.0,
        &Request {
            chart: "app",
            pin: &pin,
            document: "app",
            document_key: "config\n.toml",
            chassis: "",
        },
        &Source::Plain { opt_out: None },
    );
    assert!(refused.is_err(), "{refused:?}");
}

#[test]
fn a_chart_name_helm_would_refuse_is_refused_before_anything_is_read() {
    let tree = Tree::new();
    let pin = Pin::parse(IMAGE).expect("a pin");
    let refused = plan(
        &tree.0,
        &Request {
            chart: "App",
            pin: &pin,
            document: "app",
            document_key: "config.toml",
            chassis: "",
        },
        &Source::Plain { opt_out: None },
    );
    assert!(refused.is_err(), "{refused:?}");
}

#[test]
fn the_diff_creates_new_files_against_dev_null_and_fills_in_place() {
    let tree = Tree::new();
    tree.copied("templates/deployment.yaml", DEPLOYMENT);
    let init = run(&tree, &Source::Plain { opt_out: None }).expect("it plans");
    let diff = init.diff();
    let values = format!("{}/values.yaml", super::posix(&tree.chart()));
    assert!(
        diff.contains(&format!("--- /dev/null\n+++ {values}\n@@ -0,0 +1,")),
        "{diff}"
    );
    let deployment = format!("{}/templates/deployment.yaml", super::posix(&tree.chart()));
    assert!(
        diff.contains(&format!(
            "--- {deployment}\n+++ {deployment}\n@@ -1,4 +1,4 @@\n env:\n\
                 -  - name: %%PREFIX%%CONFIG\n"
        )),
        "{diff}"
    );
    assert!(diff.contains("+  - name: APP_CONFIG\n"), "{diff}");
}

#[test]
fn the_report_names_every_file_and_what_happens_to_it() {
    let tree = Tree::new();
    tree.copied("templates/deployment.yaml", DEPLOYMENT);
    let report = run(&tree, &Source::Plain { opt_out: None })
        .expect("it plans")
        .report();
    let chart = super::posix(&tree.chart());
    assert!(
        report.starts_with(&format!("==> {chart}: the plain form\n")),
        "{report}"
    );
    assert!(
        report.contains(&format!("    create     {chart}/values.yaml\n")),
        "{report}"
    );
    assert!(
        report.contains(&format!(
            "    fill       {chart}/templates/deployment.yaml (%%DOCUMENT_KEY%%, \
                 %%ENV_PREFIX%%, %%PREFIX%%)\n"
        )),
        "{report}"
    );
}

// --------------------------------------------------------------------------------------
// The plain form
// --------------------------------------------------------------------------------------

#[test]
fn a_third_party_image_gets_no_declaration_and_no_configuration_files() {
    let tree = Tree::new();
    tree.copied("templates/deployment.yaml", DEPLOYMENT);
    let init = run(&tree, &Source::Plain { opt_out: None }).expect("it plans");
    let mut expected = created(&[
        "values.yaml",
        "templates/_helpers.tpl",
        "ci/test-values.yaml",
    ]);
    expected.push((
        "templates/deployment.yaml".to_owned(),
        Action::Fill(vec!["DOCUMENT_KEY", "ENV_PREFIX", "PREFIX"]),
    ));
    assert_eq!(actions(&init), expected);
    assert!(init.undescribed.is_empty() && init.invented.is_empty());
    init.write().expect("it writes");
    // No dialect to read, so the chart name stands in.
    assert!(
        tree.read("templates/deployment.yaml")
            .contains("name: APP_CONFIG")
    );
    assert!(
        init.closing().contains("No configuration surface"),
        "{}",
        init.closing()
    );
}

#[test]
fn a_first_party_image_gets_the_opt_out_with_its_reason() {
    let tree = Tree::new();
    let init = run(
        &tree,
        &Source::Plain {
            opt_out: Some("The image is configured by flags alone."),
        },
    )
    .expect("it plans");
    assert_eq!(
        actions(&init),
        created(&[
            "values.yaml",
            "templates/_helpers.tpl",
            "config-contract.yaml",
            "ci/test-values.yaml",
        ])
    );
    let declaration = &init.changes[2].after;
    assert!(declaration.contains("documents: []"), "{declaration}");
    assert!(
        declaration.contains("  The image is configured by flags alone."),
        "{declaration}"
    );
    let parsed = crate::helm::declaration::parse_declaration(declaration, Path::new("app"))
        .expect("the loader accepts the opt-out");
    assert!(parsed.documents.is_empty());
    assert!(parsed.reason.is_some());
}

#[test]
fn the_closing_lists_what_the_contract_left_for_a_person() {
    let tree = Tree::new();
    let fetched = fetched();
    let closing = run(
        &tree,
        &Source::Contract {
            fetched: &fetched,
            at: "2026-01-01T00:00:00Z",
        },
    )
    .expect("it plans")
    .closing();
    for expected in [
        "2 setting(s) surfaced as chart values, 1 credential(s) delivered as files",
        "1. Add whatever the workload needs",
        "grouping block",
        "a.port",
        "`unbound` reasons",
    ] {
        assert!(closing.contains(expected), "{expected}: {closing}");
    }
}
