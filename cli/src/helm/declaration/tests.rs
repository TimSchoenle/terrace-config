use std::path::Path;

use serde_json::json;

use super::{
    Consumers, Declaration, Selection, load_declaration, parse_declaration, read_yaml,
    resolve_image, schema_violations,
};
use crate::gate::DocumentFormat;

/// One declaration read as `charts/app`'s, with nothing on disk.
fn parsed(body: &str) -> Result<Declaration, crate::error::Error> {
    parse_declaration(body, Path::new("charts/app"))
}

fn refused(body: &str) -> String {
    parsed(body)
        .expect_err("the declaration is refused")
        .to_string()
}

/// Every key the reader accepts, at one level of the file, against the schema's for that level.
fn schema_keys(pointer: &str) -> Vec<String> {
    let schema: serde_json::Value =
        serde_json::from_str(super::DECLARATION_SCHEMA).expect("the schema is JSON");
    let mut keys: Vec<String> = schema
        .pointer(pointer)
        .and_then(serde_json::Value::as_object)
        .unwrap_or_else(|| panic!("the schema has {pointer}"))
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn sorted(keys: &[&str]) -> Vec<String> {
    let mut held: Vec<String> = keys.iter().map(|key| (*key).to_owned()).collect();
    held.sort();
    held
}

#[test]
fn the_schema_names_exactly_the_keys_the_reader_accepts() {
    use super::{
        CONSUMER_KEYS, CREDENTIAL_KEYS, DECLARATION_KEYS, DOCUMENT_KEYS, EXEMPT_KEYS, IMAGE_KEYS,
        RESTART_KEYS, SOURCE_KEYS, UNBOUND_KEYS,
    };
    use crate::helm::suites::{ENTRY_KEYS, PREREQUISITE_KEYS, UNROUTED_KEYS};

    let levels: [(&str, &[&str]); 12] = [
        ("/properties", &DECLARATION_KEYS),
        ("/$defs/document/properties", &DOCUMENT_KEYS),
        ("/$defs/source/properties", &SOURCE_KEYS),
        ("/$defs/image/properties", &IMAGE_KEYS),
        ("/$defs/consumer/properties", &CONSUMER_KEYS),
        (
            "/$defs/consumer/properties/workload/properties",
            &["kind", "selector"],
        ),
        ("/$defs/exemption/properties", &EXEMPT_KEYS),
        ("/$defs/restart/properties", &RESTART_KEYS),
        ("/$defs/unbound/properties", &UNBOUND_KEYS),
        ("/$defs/credential/properties", &CREDENTIAL_KEYS),
        ("/$defs/tests/properties", &ENTRY_KEYS),
        (
            "/$defs/tests/properties/unrouted/items/properties",
            &UNROUTED_KEYS,
        ),
    ];
    for (pointer, keys) in levels {
        assert_eq!(schema_keys(pointer), sorted(keys), "{pointer}");
    }
    assert_eq!(
        schema_keys("/$defs/tests/properties/prerequisites/properties"),
        sorted(&PREREQUISITE_KEYS)
    );

    let gates: Vec<&str> = crate::gate::Relaxed::GATES
        .iter()
        .map(|(gate, _)| *gate)
        .collect();
    let schema: serde_json::Value =
        serde_json::from_str(super::DECLARATION_SCHEMA).expect("the schema is JSON");
    assert_eq!(
        schema["$defs"]["exemption"]["properties"]["gates"]["items"]["enum"],
        json!(gates)
    );
}

#[test]
fn the_schema_accepts_what_the_reader_accepts() {
    let accepted = [
        "documents:\n  - name: app\n",
        "documents: []\nreason: renders no configuration\n",
        "\
bindings: true
tests: true
unconfigured: [sidecar.image]
unbound:
  - keys: [a.b]
    reason: delivered as a file
    documents: [app]
credentials:
  - key: a.b
    value: existingSecret
documents:
  - name: app
    source:
      kind: Secret
      selector: { app: x }
      key: settings.json
      format: json
    images:
      - values: image
        contract: contracts/app.json
      - values: other.image
        contract: contracts/other.json
    consumers:
      - workload: { kind: Deployment, selector: { app: x } }
        containers: [app]
    exempt:
      - values: ci/minimal.yaml
        gates: [env, files]
        reason: renders no workload
    restart:
      - keys: [a.c]
        reason: canaried
    tests:
      probe: app.config
      baseline: { app.config.a.d: false }
      reason: the pair is refused together
      prerequisites:
        values: { existingSecret: probe }
        reason: the chart refuses to render without one
      unrouted:
        - keys: [a.e]
          reason: guarded
",
    ];
    for body in accepted {
        parsed(body).expect("the reader accepts it");
        assert_eq!(schema_violations(body), Vec::<String>::new(), "{body}");
    }
}

#[test]
fn a_contract_path_that_leaves_the_chart_directory_is_refused() {
    // `pull` writes the vendored contract at this path, so it has to stay inside the chart.
    for contract in [
        "../elsewhere.json",
        "contracts/../../x.json",
        "/etc/x.json",
        "",
    ] {
        let body = format!(
            "documents:\n  - name: server\n    images:\n      - values: image\n        \
                 contract: \"{contract}\"\n"
        );
        assert!(
            refused(&body).contains("leaves the chart directory"),
            "{contract:?} was accepted"
        );
    }
    // A defaulted path is spelled from the document's name and is held to the same rule.
    assert!(refused("documents:\n  - name: ../server\n").contains("leaves the chart directory"));

    let kept = parsed(
        "documents:\n  - name: server\n    images:\n      - values: image\n        \
             contract: ./vendor/server.json\n",
    )
    .expect("a path inside the chart is read");
    assert_eq!(kept.documents[0].images[0].contract, "./vendor/server.json");
}

#[test]
fn the_schema_refuses_what_the_reader_refuses_by_shape() {
    let refused_both = [
        "documents: []\n",
        "documents:\n  - name: app\n    typo: 1\n",
        "documents:\n  - name: app\n    source: { selector: {} }\n",
        "documents:\n  - name: app\n    images: []\n",
        "documents:\n  - name: app\n    source: { format: ini }\n",
        "documents:\n  - name: app\n    exempt:\n      - { gates: [all], reason: x }\n",
        "bindings: yes please\ndocuments:\n  - name: app\n",
        "credentials:\n  - key: a\ndocuments:\n  - name: app\n",
        "tests: true\ndocuments:\n  - name: app\n    tests: { probe: app.x }\n",
    ];
    for body in refused_both {
        parsed(body).expect_err(body);
        assert_ne!(schema_violations(body), Vec::<String>::new(), "{body}");
    }
}

fn write(body: &str) -> tempdir::Dir {
    let dir = tempdir::Dir::new();
    std::fs::write(dir.path().join("Chart.yaml"), "name: x\n").expect("the chart is written");
    std::fs::write(dir.path().join("config-contract.yaml"), body)
        .expect("the declaration is written");
    dir
}

/// A directory that removes itself, so a test that reads a real file needs no fixture tree.
mod tempdir {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    pub(super) struct Dir(PathBuf);

    impl Dir {
        pub(super) fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "terrace-contract-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).expect("a temporary directory is created");
            Self(path)
        }
        pub(super) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// A chart tree holding `alpha`, `beta` and `gamma`, and a directory that is not a chart.
fn three_charts() -> tempdir::Dir {
    let tree = tempdir::Dir::new();
    for name in ["alpha", "beta", "gamma"] {
        let chart = tree.path().join(name);
        std::fs::create_dir_all(&chart).expect("the chart directory is created");
        std::fs::write(chart.join("Chart.yaml"), format!("name: {name}\n"))
            .expect("the chart is written");
    }
    std::fs::create_dir_all(tree.path().join("scripts")).expect("the directory is created");
    tree
}

fn names(dirs: &[std::path::PathBuf]) -> Vec<&str> {
    dirs.iter()
        .filter_map(|dir| dir.file_name().and_then(std::ffi::OsStr::to_str))
        .collect()
}

#[test]
fn a_selection_naming_nothing_is_the_whole_tree() {
    let tree = three_charts();
    let dirs = Selection::all()
        .dirs(tree.path())
        .expect("the tree is walked");
    assert_eq!(names(&dirs), ["alpha", "beta", "gamma"]);
    assert_eq!(Selection::of(Vec::<String>::new()), Selection::all());
}

#[test]
fn a_selection_reads_only_the_charts_it_names_in_tree_order() {
    let tree = three_charts();
    let dirs = Selection::of(["gamma", "alpha"])
        .dirs(tree.path())
        .expect("both charts exist");
    assert_eq!(names(&dirs), ["alpha", "gamma"]);
}

#[test]
fn a_selection_naming_a_chart_the_tree_lacks_is_refused_rather_than_empty() {
    // Otherwise a typo in a pipeline selects nothing, and a gate over nothing passes.
    let tree = three_charts();
    let failure = Selection::of(["alpha", "delta", "scripts"])
        .dirs(tree.path())
        .expect_err("neither `delta` nor `scripts` is a chart")
        .to_string();
    assert!(
        failure.contains("holds no chart named delta, scripts"),
        "{failure}"
    );
}

#[test]
fn a_selected_walk_never_reads_a_chart_outside_it() {
    // `beta` carries a declaration that cannot be parsed; selecting around it must not fail.
    let tree = three_charts();
    std::fs::write(
        tree.path().join("beta").join("config-contract.yaml"),
        "documents: [\n",
    )
    .expect("the broken declaration is written");

    Selection::all()
        .declared(tree.path(), false)
        .expect_err("the whole tree reads `beta`");
    let found = Selection::of(["alpha", "gamma"])
        .declared(tree.path(), false)
        .expect("`beta` is not read");
    assert!(found.is_empty(), "neither selected chart declares anything");
}

// -- defaults --------------------------------------------------------------------------------

#[test]
fn a_document_naming_only_itself_takes_every_default() {
    let declaration = parsed("documents:\n  - name: server\n").expect("it reads");
    let document = &declaration.documents[0];

    assert_eq!(document.source.kind, "ConfigMap");
    assert_eq!(
        document.source.selector,
        vec![("app.kubernetes.io/instance".to_owned(), "app".to_owned())]
    );
    assert_eq!(document.source.key, "config.toml");
    assert_eq!(document.source.format, DocumentFormat::Toml);
    assert_eq!(document.images.len(), 1);
    assert_eq!(document.images[0].values, "image");
    assert_eq!(document.images[0].contract, "contracts/server.json");
    assert!(document.consumers.inferred());
    assert_eq!(
        document.defaulted,
        vec![
            "source.kind",
            "source.selector",
            "source.key",
            "source.format",
            "images",
            "consumers",
        ]
    );
}

#[test]
fn every_written_field_wins_over_its_default() {
    let declaration = parsed(
            "documents:\n  - name: server\n    source:\n      kind: Secret\n      \
             selector: {app.kubernetes.io/component: api}\n      key: settings.json\n      \
             format: yaml\n    images:\n      - {values: services.api.image, contract: c/a.json}\n    \
             consumers:\n      - workload: {kind: Job}\n        containers: [api]\n",
        )
        .expect("it reads");
    let document = &declaration.documents[0];

    assert_eq!(document.source.kind, "Secret");
    assert_eq!(
        document.source.selector,
        vec![("app.kubernetes.io/component".to_owned(), "api".to_owned())]
    );
    assert_eq!(document.source.key, "settings.json");
    assert_eq!(
        document.source.format,
        DocumentFormat::Yaml,
        "written beats the extension"
    );
    assert_eq!(document.images[0].values, "services.api.image");
    assert_eq!(document.images[0].contract, "c/a.json");
    assert_eq!(document.consumers.declared().len(), 1);
    assert!(document.defaulted.is_empty(), "{:?}", document.defaulted);
}

#[test]
fn the_format_follows_the_keys_extension_before_it_falls_back_to_toml() {
    for (key, format) in [
        ("config.toml", DocumentFormat::Toml),
        ("config.json", DocumentFormat::Json),
        ("config.yaml", DocumentFormat::Yaml),
        ("config.YML", DocumentFormat::Yaml),
        ("config", DocumentFormat::Toml),
        ("settings.ini", DocumentFormat::Toml),
    ] {
        let declaration = parsed(&format!(
            "documents:\n  - name: server\n    source: {{key: {key}}}\n"
        ))
        .expect("it reads");
        assert_eq!(declaration.documents[0].source.format, format, "{key}");
    }
}

#[test]
fn a_contract_is_defaulted_only_for_a_documents_one_image() {
    let declaration = parsed("documents:\n  - name: server\n    images: [{values: app.image}]\n")
        .expect("one image may leave its contract out");
    assert_eq!(
        declaration.documents[0].images[0].contract,
        "contracts/server.json"
    );
    assert!(
        declaration.documents[0]
            .defaulted
            .contains(&"images[].contract")
    );

    let error = refused(
        "documents:\n  - name: server\n    images:\n      - {values: a.image}\n      \
             - {values: b.image, contract: contracts/b.json}\n",
    );
    assert!(
        error.contains("only when the document has one image"),
        "{error}"
    );
}

#[test]
fn an_empty_selector_is_refused_rather_than_matching_every_object() {
    let error = refused("documents:\n  - name: server\n    source: {selector: {}}\n");
    assert!(error.contains("non-empty mapping of labels"), "{error}");
    assert!(error.contains("app.kubernetes.io/instance: app"), "{error}");
}

#[test]
fn an_empty_images_list_is_refused_rather_than_read_as_the_default() {
    let error = refused("documents:\n  - name: server\n    images: []\n");
    assert!(error.contains("non-empty list"), "{error}");
}

#[test]
fn consumers_are_inferred_only_for_an_object_a_pod_mounts_by_name() {
    let error = refused("documents:\n  - name: server\n    source: {kind: Settings}\n");
    assert!(error.contains("`consumers` is missing"), "{error}");

    let declaration = parsed("documents:\n  - name: server\n    consumers: []\n")
        .expect("an empty list is a written decision");
    assert!(matches!(
        &declaration.documents[0].consumers,
        Consumers::Declared(listed) if listed.is_empty()
    ));
}

#[test]
fn a_document_declared_twice_is_refused() {
    let error = refused("documents:\n  - name: server\n  - name: server\n");
    assert!(error.contains("declared twice"), "{error}");
}

#[test]
fn inferred_consumers_are_the_workloads_mounting_the_selected_object() {
    let declaration = parsed("documents:\n  - name: server\n").expect("it reads");
    let document = &declaration.documents[0];
    let manifests = vec![
        json!({"kind": "ConfigMap", "metadata": {
            "name": "renamed", "labels": {"app.kubernetes.io/instance": "app"},
        }}),
        json!({"kind": "ConfigMap", "metadata": {
            "name": "other", "labels": {"app.kubernetes.io/instance": "elsewhere"},
        }}),
        json!({"kind": "Deployment", "metadata": {"name": "app"}, "spec": {"template": {
            "spec": {
                "volumes": [{"name": "config", "configMap": {"name": "renamed"}}],
                "containers": [
                    {"name": "app", "volumeMounts": [{"name": "config", "mountPath": "/c"}]},
                ],
            },
        }}}),
        json!({"kind": "Deployment", "metadata": {"name": "unrelated"}, "spec": {"template": {
            "spec": {
                "volumes": [{"name": "config", "configMap": {"name": "other"}}],
                "containers": [
                    {"name": "x", "volumeMounts": [{"name": "config", "mountPath": "/c"}]},
                ],
            },
        }}}),
    ];

    let consumers = document.consumers_in(&manifests);
    assert_eq!(consumers.len(), 1);
    assert_eq!(consumers[0].kind, "Deployment");
    assert_eq!(consumers[0].name.as_deref(), Some("app"));
    assert_eq!(consumers[0].containers, vec!["app".to_owned()]);
    let selected = consumers[0].select(&manifests);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0]["metadata"]["name"], "app");

    assert!(
        document.consumers_in(&manifests[1..]).is_empty(),
        "no object selected, so nothing mounts it"
    );
}

#[test]
fn a_second_release_labelled_object_without_the_key_is_not_the_source() {
    let declaration = parsed("documents:\n  - name: server\n").expect("it reads");
    let document = &declaration.documents[0];
    let mounted = |workload: &str, container: &str, object: &str| {
        json!({"kind": "Deployment", "metadata": {"name": workload}, "spec": {"template": {
            "spec": {
                "volumes": [{"name": "v", "configMap": {"name": object}}],
                "containers": [
                    {"name": container, "volumeMounts": [{"name": "v", "mountPath": "/c"}]},
                ],
            },
        }}})
    };
    let manifests = vec![
        json!({"kind": "ConfigMap", "data": {"config.toml": ""}, "metadata": {
            "name": "app", "labels": {"app.kubernetes.io/instance": "app"},
        }}),
        json!({"kind": "ConfigMap", "data": {"run.sh": ""}, "metadata": {
            "name": "app-scripts", "labels": {"app.kubernetes.io/instance": "app"},
        }}),
        mounted("app", "app", "app"),
        mounted("app-migrate", "migrate", "app-scripts"),
    ];

    let consumers = document.consumers_in(&manifests);
    assert_eq!(consumers.len(), 1, "{consumers:?}");
    assert_eq!(consumers[0].name.as_deref(), Some("app"));
}

// -- round-trip enrolment ---------------------------------------------------------------------

#[test]
fn an_enrolled_chart_gives_every_document_a_suite_and_its_block_says_how() {
    let declaration = parsed(
        "tests: true\ndocuments:\n  - name: server\n    tests:\n      prerequisites:\n        \
             values: {existingSecret: probe}\n        reason: the render guard wants a secret\n  \
             - name: worker\n",
    )
    .expect("it reads");
    assert!(declaration.tests);
    let server = declaration.documents[0].tests.as_ref().expect("enrolled");
    assert_eq!(
        server.prerequisites.values,
        vec![("existingSecret".to_owned(), json!("probe"))]
    );
    let worker = declaration.documents[1]
        .tests
        .as_ref()
        .expect("enrolled by default");
    assert!(
        worker.prerequisites.values.is_empty(),
        "a document with no block takes the defaults"
    );

    let unenrolled = parsed("documents:\n  - name: server\n").expect("it reads");
    assert!(!unenrolled.tests);
    assert!(unenrolled.documents[0].tests.is_none());
}

#[test]
fn a_tests_block_on_a_chart_that_is_not_enrolled_is_refused() {
    let error =
        refused("documents:\n  - name: server\n    tests:\n      probe: config\n      reason: x\n");
    assert!(error.contains("set `tests: true`"), "{error}");
}

#[test]
fn a_tests_block_is_read_under_the_enrolment_files_rules() {
    let error = refused("tests: true\ndocuments:\n  - name: server\n    tests: {name: x}\n");
    assert!(error.contains("unknown key(s) name"), "{error}");

    let error = refused(
        "tests: true\ndocuments:\n  - name: server\n    tests:\n      \
             baseline: {config.a: 1}\n",
    );
    assert!(error.contains("has no `reason`"), "{error}");

    let error = refused("tests: yes please\ndocuments:\n  - name: server\n");
    assert!(error.contains("must be true or false"), "{error}");
}

#[test]
fn a_key_nobody_reads_is_a_typo_rather_than_something_to_ignore() {
    let dir = write("documents: []\nreason: not adopted\nunbounded: []\n");
    let error = load_declaration(dir.path()).expect_err("an unknown key is refused");
    assert!(
        error.to_string().contains("unknown key(s) unbounded"),
        "{error}"
    );
}

#[test]
fn an_empty_document_list_is_an_opt_out_and_needs_a_reason() {
    let dir = write("documents: []\n");
    let error = load_declaration(dir.path()).expect_err("an unexplained opt-out is refused");
    assert!(error.to_string().contains("explicit opt-out"), "{error}");

    let dir = write("documents: []\nreason: the image publishes none yet\n");
    let declaration = load_declaration(dir.path())
        .expect("an explained opt-out reads")
        .expect("the chart has a declaration");
    assert!(declaration.documents.is_empty());
    assert_eq!(
        declaration.reason.as_deref(),
        Some("the image publishes none yet")
    );
}

#[test]
fn a_chart_with_no_declaration_is_skipped_rather_than_failed() {
    let dir = tempdir::Dir::new();
    assert!(
        load_declaration(dir.path())
            .expect("no declaration is not an error")
            .is_none()
    );
}

#[test]
fn an_exemption_names_a_gate_that_exists_and_says_why() {
    let dir = write(
        "documents:\n  - name: app\n    source: {kind: ConfigMap, key: config.toml}\n    \
             images: [{values: image, contract: contracts/app.json}]\n    exempt:\n      \
             - {values: 'ci/a.yaml', gates: [gate2], reason: x}\n",
    );
    let error = load_declaration(dir.path()).expect_err("a gate that does not exist is refused");
    assert!(
        error.to_string().contains("gates that do not exist: gate2"),
        "{error}"
    );

    let dir = write(
        "documents:\n  - name: app\n    source: {kind: ConfigMap, key: config.toml}\n    \
             images: [{values: image, contract: contracts/app.json}]\n    exempt:\n      \
             - {values: 'ci/a.yaml', gates: [env]}\n",
    );
    let error = load_declaration(dir.path()).expect_err("an unexplained exemption is refused");
    assert!(
        error
            .to_string()
            .contains("indistinguishable from an oversight"),
        "{error}"
    );
}

#[test]
fn an_exemption_applies_to_the_values_file_it_names_and_a_star_to_every_one() {
    let dir = write(
        "documents:\n  - name: app\n    source: {kind: ConfigMap, key: config.toml}\n    \
             images: [{values: image, contract: contracts/app.json}]\n    exempt:\n      \
             - {values: 'ci/a.yaml', gates: [env], reason: r}\n      \
             - {values: '*', gates: [closed], reason: r}\n",
    );
    let declaration = load_declaration(dir.path())
        .expect("the declaration reads")
        .expect("the chart has one");
    let document = &declaration.documents[0];
    assert!(document.relaxed("a.yaml").env);
    assert!(!document.relaxed("b.yaml").env);
    assert!(document.relaxed("b.yaml").closed);
}

#[test]
fn two_reasons_for_one_unbound_key_means_one_of_them_is_not_the_reason() {
    let dir = write(
        "documents: []\nreason: r\nunbound:\n  - {keys: [a], reason: one}\n  \
             - {keys: [a], reason: two}\n",
    );
    let error = load_declaration(dir.path()).expect_err("a key written off twice is refused");
    assert!(error.to_string().contains("already written off"), "{error}");
}

#[test]
fn an_unbound_scope_must_name_a_document_the_chart_declares() {
    let dir = write(
        "documents:\n  - name: app\n    source: {kind: ConfigMap, key: config.toml}\n    \
             images: [{values: image, contract: contracts/app.json}]\nunbound:\n  \
             - {keys: [a], reason: r, documents: [other]}\n",
    );
    let error = load_declaration(dir.path()).expect_err("an unknown document is refused");
    assert!(error.to_string().contains("does not declare"), "{error}");
}

#[test]
fn a_credential_row_adds_something_or_is_not_written() {
    let dir = write("documents: []\nreason: r\ncredentials:\n  - {key: a}\n");
    let error = load_declaration(dir.path()).expect_err("an empty row is refused");
    assert!(error.to_string().contains("adds neither"), "{error}");
}

#[test]
fn an_image_reference_falls_back_to_the_charts_app_version() {
    let values = json!({"image": {"repository": "ghcr.io/x/y"}});
    let pinned =
        resolve_image(&values, "image", Some("v1.2.3@sha256:ab")).expect("the path resolves");
    assert_eq!(pinned.reference, "ghcr.io/x/y:v1.2.3@sha256:ab");
    assert_eq!(pinned.normalized, "ghcr.io/x/y");
    assert_eq!(pinned.digest.as_deref(), Some("sha256:ab"));
}

#[test]
fn a_values_path_that_pins_nothing_is_refused_by_name() {
    let error = resolve_image(&json!({}), "image", None).expect_err("nothing resolves");
    assert!(error.to_string().contains("'image'"), "{error}");
}

#[test]
fn an_empty_file_reads_as_an_empty_mapping() {
    let empty = read_yaml("", Path::new("x")).expect("an empty file reads");
    assert_eq!(empty, json!({}));
    assert_eq!(
        read_yaml("---\n", Path::new("x")).expect("a null document reads"),
        json!({})
    );
}
