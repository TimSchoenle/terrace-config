use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value as Json, json};

use super::{ENROLMENT, Enrolment, discriminator_for, load_enrolment, orphans, probe_tree};
use crate::gate::document::{DocumentFormat, DocumentSource};
use crate::helm::DECLARATION;
use crate::helm::declaration::load_declaration;
use crate::helm::declaration::{Consumers, Declaration, Document, ImageRef};
use crate::helm::testgen::VALUES_ROOT;

/// A chart directory holding the files a test wants, removed when it goes out of scope.
///
/// Hand-rolled rather than a dependency: several of these need a directory on disk, and the
/// crate that would supply one would be carried by every consumer of this library's test build.
struct Chart(PathBuf);

impl Chart {
    fn of(files: &[(&str, &str)]) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let at = std::env::temp_dir().join(format!(
            "terrace-suites-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(&at).expect("a directory");
        for (name, body) in files {
            let path = at.join(name);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(path, body).expect("a file is written");
        }
        Self(at)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Chart {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One enrolment file, read.
fn enrolment(body: &str) -> Result<BTreeMap<String, Enrolment>, crate::error::Error> {
    let chart = Chart::of(&[(ENROLMENT, body)]);
    Ok(load_enrolment(chart.path(), None)?
        .expect("the file is there")
        .documents)
}

fn refused(body: &str) -> String {
    enrolment(body)
        .expect_err("the enrolment is refused")
        .to_string()
}

/// One declared document, as a name, a source key and the labels it is selected on.
type Declared<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

/// One chart's declaration, from those triples.
fn declaration(documents: &[Declared<'_>]) -> Declaration {
    Declaration {
        chart: "chart".to_owned(),
        path: PathBuf::from("charts/chart/config-contract.yaml"),
        documents: documents
            .iter()
            .map(|(name, key, selector)| Document {
                name: (*name).to_owned(),
                source: DocumentSource {
                    kind: "ConfigMap".to_owned(),
                    selector: selector
                        .iter()
                        .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
                        .collect(),
                    key: (*key).to_owned(),
                    format: DocumentFormat::Toml,
                },
                images: vec![ImageRef {
                    values: "image".to_owned(),
                    contract: "contracts/one.json".to_owned(),
                }],
                consumers: Consumers::Declared(Vec::new()),
                exempt: Vec::new(),
                restart: Vec::new(),
                tests: None,
                defaulted: Vec::new(),
            })
            .collect(),
        reason: None,
        unconfigured: Vec::new(),
        bindings: false,
        unbound: Vec::new(),
        credentials: BTreeMap::new(),
        tests: false,
    }
}

// -- the enrolment file ------------------------------------------------------------------

#[test]
fn a_chart_without_the_file_is_not_enrolled() {
    let chart = Chart::of(&[]);
    assert!(
        load_enrolment(chart.path(), None)
            .expect("an absent file reads")
            .is_none()
    );
    assert!(
        enrolment("documents: []")
            .expect("an empty list is legal")
            .is_empty()
    );
}

#[test]
fn a_declaration_that_enrols_the_chart_is_the_enrolment_and_names_itself() {
    let body = "tests: true\ndocuments:\n  - name: server\n    tests:\n      probe: config\n";
    let chart = Chart::of(&[(DECLARATION, body)]);
    let declaration = load_declaration(chart.path())
        .expect("it reads")
        .expect("it is there");
    let enrolments = load_enrolment(chart.path(), Some(&declaration))
        .expect("it reads")
        .expect("the chart is enrolled");
    assert_eq!(enrolments.path, chart.path().join(DECLARATION));
    assert_eq!(enrolments.documents.keys().collect::<Vec<_>>(), ["server"]);

    let unenrolled = Chart::of(&[(DECLARATION, "documents:\n  - name: server\n")]);
    let declaration = load_declaration(unenrolled.path())
        .expect("it reads")
        .expect("it is there");
    assert!(
        load_enrolment(unenrolled.path(), Some(&declaration))
            .expect("it reads")
            .is_none()
    );
}

#[test]
fn a_chart_enrolled_both_ways_is_refused() {
    let chart = Chart::of(&[
        (DECLARATION, "tests: true\ndocuments:\n  - name: server\n"),
        (ENROLMENT, "documents: []\n"),
    ]);
    let declaration = load_declaration(chart.path())
        .expect("it reads")
        .expect("it is there");
    let error = load_enrolment(chart.path(), Some(&declaration))
        .expect_err("two enrolments are refused")
        .to_string();
    assert!(error.contains("enrols this chart a second time"), "{error}");
    assert!(error.contains(ENROLMENT), "{error}");
}

#[test]
fn a_baseline_is_read_as_sorted_pairs() {
    let enrolled = enrolment(
        "documents:\n  - name: server\n    baseline:\n      config.b: 1\n      \
             config.a: false\n    reason: because\n",
    )
    .expect("it reads");
    assert_eq!(
        enrolled["server"].baseline.values,
        [
            ("config.a".to_owned(), json!(false)),
            ("config.b".to_owned(), json!(1)),
        ]
    );
}

#[test]
fn a_baseline_without_a_reason_is_refused() {
    // An unexplained hole in a gate is indistinguishable from an oversight.
    let failure = refused("documents:\n  - name: server\n    baseline:\n      config.a: false\n");
    assert!(failure.contains("reason"), "{failure}");
}

#[test]
fn a_baseline_that_is_not_configuration_at_the_probed_layer_is_refused() {
    // Outside the tree it would never compare equal to a probe, so the collision it exists to
    // be dropped for would go unnoticed. Nested, the comparison could not be made at all.
    assert!(
        refused(
            "documents:\n  - name: server\n    baseline:\n      image.tag: v1\n    \
                 reason: because\n"
        )
        .contains("under `config`")
    );
    assert!(
        refused(
            "documents:\n  - name: server\n    baseline:\n      config.csp: {a: 1}\n    \
                 reason: because\n"
        )
        .contains("not a scalar")
    );
}

#[test]
fn a_document_with_no_probe_is_probed_through_the_configuration_tree() {
    let enrolled = enrolment("documents:\n  - name: server\n").expect("it reads");
    assert_eq!(enrolled["server"].probe, VALUES_ROOT);
}

#[test]
fn a_probe_path_is_read_as_the_root_every_case_writes_under() {
    let enrolled = enrolment(
        "documents:\n  - name: api\n    probe: services.api.config\n    reason: because\n",
    )
    .expect("it reads");
    assert_eq!(enrolled["api"].probe, "services.api.config");
}

#[test]
fn a_probe_that_is_not_a_dotted_values_path_is_refused() {
    // It is used both as a `set` prefix and as a walk into the chart's values, and neither
    // reads anything else.
    for offender in [
        "'services[\"api\"].config'",
        "'services..config'",
        "'.config'",
        "''",
        "7",
    ] {
        let body =
            format!("documents:\n  - name: api\n    probe: {offender}\n    reason: because\n");
        assert!(enrolment(&body).is_err(), "{offender}");
    }
}

#[test]
fn a_probe_without_a_reason_is_refused() {
    // It narrows the suite to one layer, which is the same hole a baseline opens.
    let failure = refused("documents:\n  - name: api\n    probe: services.api.config\n");
    assert!(failure.contains("reason"), "{failure}");
}

#[test]
fn a_baseline_is_read_under_the_probe_root_rather_than_under_the_default() {
    let enrolled = enrolment(
        "documents:\n  - name: api\n    probe: services.api.config\n    baseline:\n      \
             services.api.config.a: false\n    reason: because\n",
    )
    .expect("it reads");
    assert_eq!(
        enrolled["api"].baseline.values,
        [("services.api.config.a".to_owned(), json!(false))]
    );

    assert!(
        enrolment(
            "documents:\n  - name: api\n    probe: services.api.config\n    baseline:\n      \
                 config.a: false\n    reason: because\n",
        )
        .is_err()
    );
}

#[test]
fn a_typo_is_refused_rather_than_ignored() {
    // The same rule the declaration is read under: a key nobody reads is a key somebody meant
    // to spell differently.
    assert!(enrolment("documents: []\nreason: stray").is_err());
    assert!(enrolment("documents:\n  - name: server\n  - name: server\n").is_err());
}

// -- render prerequisites ----------------------------------------------------------------

fn prerequisites(values: &str) -> Result<BTreeMap<String, Enrolment>, crate::error::Error> {
    enrolment(&format!(
        "documents:\n  - name: server\n    prerequisites:\n      values:\n{values}      \
             reason: because the guard insists\n"
    ))
}

#[test]
fn prerequisites_are_read_as_sorted_pairs_with_their_shapes_intact() {
    // A map keyed by a request path reads worse split up than it does whole, and a flattened
    // one would set the wrong thing.
    let enrolled = prerequisites(
        "        webhook.targetBase: https://example.invalid\n        bucket.entries:\n          \
             link: {bucket: b, object: o}\n",
    )
    .expect("it reads");
    assert_eq!(
        enrolled["server"].prerequisites.values,
        [
            (
                "bucket.entries".to_owned(),
                json!({"link": {"bucket": "b", "object": "o"}})
            ),
            (
                "webhook.targetBase".to_owned(),
                json!("https://example.invalid")
            ),
        ]
    );
    assert_eq!(
        enrolled["server"].prerequisites.reason.as_deref(),
        Some("because the guard insists")
    );
}

#[test]
fn a_prerequisite_inside_the_probed_tree_is_refused_with_the_file_named() {
    // The guarantee, at the loader; the model refuses it again for a caller that skips this.
    let failure = prerequisites("        config.isr.ttl_secs: 99\n")
        .expect_err("it is refused")
        .to_string();
    assert!(failure.contains(ENROLMENT), "{failure}");
    assert!(failure.contains(VALUES_ROOT), "{failure}");
    assert!(prerequisites("        config: {isr: {ttl_secs: 99}}\n").is_err());
}

#[test]
fn a_prerequisite_block_that_explains_or_sets_nothing_is_refused() {
    // A value nobody explained is one nobody can tell from a workaround somebody stopped
    // needing, and a block that sets nothing is a field somebody meant to fill in.
    assert!(
        enrolment(
            "documents:\n  - name: server\n    prerequisites:\n      values:\n        \
                 webhook.targetBase: https://example.invalid\n",
        )
        .expect_err("it is refused")
        .to_string()
        .contains("reason")
    );
    assert!(
        enrolment("documents:\n  - name: server\n    prerequisites:\n      reason: because\n")
            .is_err()
    );
    assert!(
        enrolment(
            "documents:\n  - name: server\n    prerequisites:\n      value: {a: 1}\n      \
                 reason: because\n",
        )
        .is_err()
    );
}

#[test]
fn overlapping_prerequisites_are_refused() {
    // A `set` mapping has no order, so which of the two survives is stated nowhere.
    let failure =
        prerequisites("        bucket: {}\n        bucket.entries:\n          link: {bucket: b}\n")
            .expect_err("it is refused")
            .to_string();
    assert!(failure.contains("overlap"), "{failure}");
}

#[test]
fn a_moved_probe_root_moves_which_prerequisite_the_loader_refuses() {
    // The loader reads the root first, so its refusal is the model's under a moved root.
    let enrolled = enrolment(
        "documents:\n  - name: api\n    probe: services.api.config\n    reason: the derived \
             wiring outranks it\n    prerequisites:\n      values:\n        config.profile: \
             development\n      reason: because the guard insists\n",
    )
    .expect("it reads");
    assert_eq!(
        enrolled["api"].prerequisites.values,
        [("config.profile".to_owned(), json!("development"))]
    );

    let failure = enrolment(
        "documents:\n  - name: api\n    probe: services.api.config\n    reason: the derived \
             wiring outranks it\n    prerequisites:\n      values:\n        \
             services.api.config.profile: development\n      reason: because the guard insists\n",
    )
    .expect_err("it is refused")
    .to_string();
    assert!(failure.contains("services.api.config"), "{failure}");
}

#[test]
fn a_document_may_carry_a_baseline_and_prerequisites_at_once() {
    let enrolled = enrolment(
        "documents:\n  - name: server\n    baseline:\n      config.a: false\n    reason: \
             the chart's own guard\n    prerequisites:\n      values:\n        webhook.targetBase: \
             https://example.invalid\n      reason: the other guard\n",
    )
    .expect("it reads");
    assert_eq!(
        enrolled["server"].baseline.values,
        [("config.a".to_owned(), json!(false))]
    );
    assert_eq!(
        enrolled["server"].prerequisites.values,
        [(
            "webhook.targetBase".to_owned(),
            json!("https://example.invalid")
        )]
    );
}

// -- unrouted keys -----------------------------------------------------------------------

#[test]
fn an_unrouted_entry_states_its_keys_and_one_reason_for_them() {
    let enrolled = enrolment(
        "documents:\n  - name: server\n    unrouted:\n      - keys: [b.two, a.one]\n        \
             reason: the chart guards the position\n",
    )
    .expect("it reads");
    assert_eq!(
        enrolled["server"].unrouted,
        [
            (
                "a.one".to_owned(),
                "the chart guards the position".to_owned()
            ),
            (
                "b.two".to_owned(),
                "the chart guards the position".to_owned()
            ),
        ]
    );
}

#[test]
fn an_unrouted_entry_with_no_reason_or_a_key_named_twice_is_refused() {
    // A probe moved off the chart's own value without a reason is indistinguishable from an
    // oversight, and two reasons for one key means one of them is stale.
    assert!(
        enrolment("documents:\n  - name: server\n    unrouted:\n      - keys: [a.one]\n").is_err()
    );
    assert!(
        enrolment(
            "documents:\n  - name: server\n    unrouted:\n      - keys: [a.one]\n        \
                 reason: one\n      - keys: [a.one]\n        reason: two\n",
        )
        .expect_err("it is refused")
        .to_string()
        .contains("stale")
    );
}

// -- what a chart has to expose ----------------------------------------------------------

#[test]
fn a_probe_path_the_chart_does_not_expose_is_refused() {
    // The likelier mistake by far: an enrolment naming a path this chart does not have, which
    // would otherwise generate a whole suite of cases setting values nothing reads.
    let values: Json = json!({"config": {}, "services": {"api": {"config": {}}}});
    let chart = Chart::of(&[]);
    probe_tree(chart.path(), &values, "api", "services.api.config").expect("it is there");
    for missing in ["services.worker.config", "nothing"] {
        assert!(
            probe_tree(chart.path(), &values, "api", missing).is_err(),
            "{missing}"
        );
    }
}

// -- telling one document from its siblings ----------------------------------------------

#[test]
fn the_key_is_derived_to_be_enough_when_no_sibling_shares_it() {
    let held = declaration(&[
        (
            "server",
            "config.toml",
            &[("app.kubernetes.io/instance", "portfolio")],
        ),
        (
            "agent",
            "agent.toml",
            &[("app.kubernetes.io/instance", "portfolio")],
        ),
    ]);
    for document in &held.documents {
        assert!(
            discriminator_for(&held, document)
                .expect("it is derivable")
                .is_empty(),
            "{}",
            document.name
        );
    }
}

#[test]
fn a_shared_key_derives_the_labels_from_the_declaration() {
    // A second copy in the enrolment could disagree with the first, and the gate reading the
    // first would then be validating an object the suite never selects.
    let held = declaration(&[
        (
            "api",
            "config.toml",
            &[("app.kubernetes.io/component", "api")],
        ),
        (
            "worker",
            "config.toml",
            &[("app.kubernetes.io/component", "worker")],
        ),
    ]);
    assert_eq!(
        discriminator_for(&held, &held.documents[0]).expect("it is derivable"),
        [("app.kubernetes.io/component".to_owned(), "api".to_owned())]
    );
}

#[test]
fn a_shared_key_nothing_tells_apart_is_refused() {
    // Deriving them anyway would emit a selector that matches both and fails at run time.
    let held = declaration(&[("api", "config.toml", &[]), ("worker", "config.toml", &[])]);
    let failure = discriminator_for(&held, &held.documents[0])
        .expect_err("it is refused")
        .to_string();
    assert!(failure.contains("tells the documents apart"), "{failure}");

    let shared: &[(&str, &str)] = &[("app.kubernetes.io/instance", "chart")];
    let held = declaration(&[
        ("api", "config.toml", shared),
        ("worker", "config.toml", shared),
    ]);
    assert!(discriminator_for(&held, &held.documents[0]).is_err());
}

// -- suites left behind ------------------------------------------------------------------

fn sweep(wanted: &[&str], present: &[&str]) -> Vec<String> {
    let files: Vec<(String, &str)> = present
        .iter()
        .map(|name| (format!("tests/{name}"), "suite: x\n"))
        .collect();
    let held: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, body)| (name.as_str(), *body))
        .collect();
    let chart = Chart::of(&held);
    let wanted: BTreeMap<PathBuf, String> = wanted
        .iter()
        .map(|name| (chart.path().join("tests").join(name), String::new()))
        .collect();
    orphans(chart.path(), &wanted)
        .into_iter()
        .map(|path| {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

#[test]
fn a_suite_whose_document_is_gone_is_reported() {
    // It keeps asserting keys nothing reads any more, which is the drift to catch.
    assert_eq!(
        sweep(
            &["contract_roundtrip_api_test.yaml"],
            &[
                "contract_roundtrip_api_test.yaml",
                "contract_roundtrip_worker_test.yaml"
            ]
        ),
        ["contract_roundtrip_worker_test.yaml"]
    );
}

#[test]
fn un_enrolling_a_chart_orphans_every_suite_it_had_and_touches_nothing_else() {
    assert_eq!(
        sweep(&[], &["contract_roundtrip_api_test.yaml"]),
        ["contract_roundtrip_api_test.yaml"]
    );
    assert_eq!(sweep(&[], &["configmap_test.yaml"]), Vec::<String>::new());
}
