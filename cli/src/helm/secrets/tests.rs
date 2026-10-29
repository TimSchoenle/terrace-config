use serde_json::{Value as Json, json};

use super::{
    Credential, Declared, Ledger, Mount, Where, by_container, common_prefix, credentials,
    gate_three_reach, names_credential, scan_file,
};
use crate::gate::container::ContainerView;
use crate::gate::document::DocumentSource;
use crate::helm::declaration::{Consumer, Consumers, Declaration, Document};
use crate::k8s::secret_file_names;
use crate::union::{Union, union_contracts};

/// The two fixtures every scope question here is asked against: one image's contract, and a
/// sibling's. A file only the sibling declares is over-projection; a file neither declares is
/// unclaimed, and collapsing the two into a union would erase the distinction the whole report
/// rests on.
const API: &str = include_str!("../../../tests/fixtures/api.json");
const WORKER: &str = include_str!("../../../tests/fixtures/worker.json");

fn union(label: &str, contract: &str) -> Union {
    union_contracts(&[(
        label.to_owned(),
        serde_json::from_str(contract).expect("a fixture contract"),
    )])
    .expect("one contract merges with itself")
}

/// The `api` fixture plus one `text` key the image does *not* consider secret.
///
/// No fixture on disk carries that combination, and it is the exact shape of a key a chart
/// elevates: a value the image treats as ordinary configuration and the chart delivers as a
/// Secret file anyway.
fn with_plain_text_key() -> Union {
    let mut contract: Json = serde_json::from_str(API).expect("a fixture contract");
    contract["schema"]["keys"]
        .as_array_mut()
        .expect("a list of keys")
        .push(json!({
            "path": "email.username",
            "env": "FIXTURE_EMAIL__USERNAME",
            "env_file": "FIXTURE_EMAIL__USERNAME_FILE",
            "secrets_file": "email__username",
            "docs": "The mailbox login.",
            "ty": "String",
            "values": [],
            "constraint": {"type": "string"},
            "text_constraint": null,
            "text_form": "text",
            "aliases": [],
            "default": null,
            "default_value": null,
            "note": null,
            "required": false,
            "secret": false,
            "reserved": false,
        }));
    union_contracts(&[("api".to_owned(), contract)]).expect("one contract merges with itself")
}

fn declaration(documents: Vec<Document>) -> Declaration {
    Declaration {
        chart: "fixture".to_owned(),
        path: std::path::PathBuf::from("fixture"),
        documents,
        reason: None,
        unconfigured: Vec::new(),
        bindings: false,
        unbound: Vec::new(),
        credentials: std::collections::BTreeMap::new(),
        tests: false,
    }
}

fn container(env: &[(&str, &str)]) -> Json {
    json!({
        "name": "app",
        "image": format!("example/app:v1@sha256:{}", "0".repeat(64)),
        "env": env
            .iter()
            .map(|(name, value)| json!({"name": name, "value": value}))
            .collect::<Vec<_>>(),
        "volumeMounts": [],
    })
}

// -- what a volume delivers ------------------------------------------------------------

#[test]
fn a_projected_volume_reports_only_its_secret_sources() {
    // `config.toml` arriving from a ConfigMap in the same volume is not a credential, and
    // counting it would make every pod in a repository look wrong at once.
    let spec = json!({
        "volumes": [{
            "name": "config",
            "projected": {"sources": [
                {"configMap": {"name": "cm", "items": [{"key": "config.toml"}]}},
                {"secret": {"name": "s", "items": [{"key": "k", "path": "db__url"}]}},
            ]},
        }]
    });
    assert_eq!(secret_file_names(&[], &spec, "config"), ["db__url"]);
}

#[test]
fn a_source_without_items_presents_every_key_of_the_rendered_secret() {
    let spec = json!({"volumes": [{"name": "creds", "secret": {"secretName": "existing"}}]});
    let manifests = [json!({
        "kind": "Secret",
        "metadata": {"name": "existing"},
        "data": {"database__url": "eA=="},
        "stringData": {"auth__jwt_secret": "x"},
    })];
    assert_eq!(
        secret_file_names(&manifests, &spec, "creds"),
        ["auth__jwt_secret", "database__url"]
    );
}

#[test]
fn a_secret_the_operator_supplies_contributes_no_names() {
    // Nothing renders it, so nothing can be read from it. Silence is the only honest answer.
    let spec = json!({"volumes": [{"name": "creds", "secret": {"secretName": "not-rendered"}}]});
    assert_eq!(secret_file_names(&[], &spec, "creds"), Vec::<String>::new());
}

#[test]
fn a_volume_the_pod_does_not_have_is_not_an_error() {
    assert_eq!(
        secret_file_names(&[], &json!({"volumes": []}), "missing"),
        Vec::<String>::new()
    );
}

// -- the inventory ---------------------------------------------------------------------

fn declared(document: &str, contract: &str, image: &str, required: bool) -> Declared {
    Declared {
        chart: "c".to_owned(),
        document: document.to_owned(),
        contract: contract.to_owned(),
        image: image.to_owned(),
        path: "database.url".to_owned(),
        secrets_file: "database__url".to_owned(),
        env: "FIXTURE_DATABASE__URL".to_owned(),
        env_file: "FIXTURE_DATABASE__URL_FILE".to_owned(),
        text_form: "text".to_owned(),
        required,
        entries: Vec::new(),
        summary: "The connection string.".to_owned(),
    }
}

#[test]
fn two_images_reading_one_key_are_one_row_naming_both() {
    let rows = credentials(&[
        declared("api", "c/contracts/api.json", "api", false),
        declared("worker", "c/contracts/worker.json", "worker", false),
    ]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].images, ["api", "worker"]);
    assert_eq!(rows[0].documents, ["api", "worker"]);
}

#[test]
fn required_unions_the_way_the_contract_model_unions_it() {
    // A key any reader requires is a key the deployment must carry.
    let rows = credentials(&[
        declared("d", "c/contracts/a.json", "a", false),
        declared("d", "c/contracts/b.json", "b", true),
    ]);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].required);
}

#[test]
fn two_spellings_of_one_path_stay_two_rows() {
    // Two files on disk and two variables. Collapsing them would name an artefact that does not
    // exist, which is the opposite of what an inventory is for.
    let mut renamed = declared("d", "c/contracts/b.json", "b", false);
    renamed.secrets_file = "db__url".to_owned();
    let rows = credentials(&[declared("d", "c/contracts/a.json", "a", false), renamed]);
    assert_eq!(rows.len(), 2);
}

// -- which files of a chart name a credential -------------------------------------------

fn credential(path: &str, secrets_file: &str) -> Credential {
    Credential {
        chart: "c".to_owned(),
        path: path.to_owned(),
        secrets_file: secrets_file.to_owned(),
        env: "TANKOVAULT_INTERNAL__TOKEN".to_owned(),
        env_file: "TANKOVAULT_INTERNAL__TOKEN_FILE".to_owned(),
        required: false,
        entries: Vec::new(),
        summary: String::new(),
        documents: vec!["d".to_owned()],
        images: vec!["a".to_owned()],
        contracts: vec!["c/contracts/a.json".to_owned()],
    }
}

/// A chart directory with the files a test wants in it, removed when it goes out of scope.
///
/// Hand-rolled rather than a dependency: three of these tests need a directory on disk, and the
/// crate that would supply one would be carried by every consumer of this library's test build.
struct Chart(std::path::PathBuf);

impl Chart {
    fn of(files: &[(&str, &str)]) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let at = std::env::temp_dir().join(format!(
            "terrace-secrets-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&at);
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

#[test]
fn a_longer_spelling_that_contains_the_shorter_one_does_not_name_it() {
    // `internal__tokens__api` contains `internal__token`, so a substring search calls the retired
    // tier-wide key deliverable on the strength of an unrelated one, and downgrades the only
    // finding that mattered.
    let chart = Chart::of(&[(
        "templates/_secrets.tpl",
        "key: {{ printf \"internal__tokens__%s\" . }}\n{{ .Values.internal.tokens }}\n",
    )]);
    assert_eq!(
        names_credential(
            chart.path(),
            &credential("internal.token", "internal__token")
        )
        .expect("the chart is readable"),
        Vec::<String>::new()
    );
}

#[test]
fn the_spelling_itself_names_it() {
    let chart = Chart::of(&[("values.yaml", "# internal.token is gone\n")]);
    assert_eq!(
        names_credential(
            chart.path(),
            &credential("internal.token", "internal__token")
        )
        .expect("the chart is readable"),
        ["values.yaml"]
    );
}

#[test]
fn the_vendored_contracts_are_not_evidence_that_the_chart_delivers_it() {
    // They are the other side of the comparison: every credential is named in one by
    // definition, so counting them would make the question unanswerable.
    let chart = Chart::of(&[(
        "contracts/a.json",
        "{\"secrets_file\": \"internal__token\"}",
    )]);
    assert_eq!(
        names_credential(
            chart.path(),
            &credential("internal.token", "internal__token")
        )
        .expect("the chart is readable"),
        Vec::<String>::new()
    );
}

// -- one mounted file, which is where each of the three reports is decided ---------------

/// One call of [`scan_file`], with everything a test does not care about held constant.
struct Scan {
    mine: Union,
    sibling: Union,
    ledger: Ledger,
    surface: super::Surface,
}

impl Scan {
    fn new() -> Self {
        Self {
            mine: union("worker", WORKER),
            sibling: union("api", API),
            ledger: Ledger::default(),
            surface: super::Surface::default(),
        }
    }

    fn scan(
        &mut self,
        file_name: &str,
        mount_path: &str,
        secrets_dir: Option<&str>,
        judged: &[(&str, &str)],
        env: &[(&str, &str)],
    ) {
        let held = container(env);
        let view = ContainerView::read(&[], &held, &self.mine);
        let contracts = [&self.mine, &self.sibling];
        let judged = judged
            .iter()
            .map(|(workload, container)| ((*workload).to_owned(), (*container).to_owned()))
            .collect();
        scan_file(
            &declaration(Vec::new()),
            &contracts,
            &self.mine,
            &view,
            secrets_dir,
            Where {
                values_file: "default.yaml",
                workload: "Deployment app",
                container: "app",
            },
            mount_path,
            file_name,
            &judged,
            &mut self.ledger,
            &mut self.surface,
        );
    }

    fn supplied(&self) -> Vec<(&str, &str, Vec<&str>)> {
        self.ledger
            .supplied
            .iter()
            .map(|((contract, path), channels)| {
                (
                    contract.as_str(),
                    path.as_str(),
                    channels.iter().map(String::as_str).collect(),
                )
            })
            .collect()
    }
}

#[test]
fn a_key_the_container_reads_is_recorded_as_supplied_and_reported_nowhere() {
    let mut scan = Scan::new();
    scan.scan("database__url", "/secrets", Some("/secrets"), &[], &[]);
    assert_eq!(
        scan.supplied(),
        [("worker", "database.url", vec!["the secrets directory"])]
    );
    assert_eq!(scan.surface.unclaimed, Vec::new());
    assert_eq!(scan.surface.over_projected, Vec::new());
}

#[test]
fn a_file_outside_the_secrets_directory_supplies_nothing_without_indirection() {
    // Nothing opens it. A loader reads the directory it was told about and the files a `_FILE`
    // variable names, and a file lying anywhere else is a file lying anywhere else.
    let mut scan = Scan::new();
    scan.scan("database__url", "/elsewhere", Some("/secrets"), &[], &[]);
    assert_eq!(scan.supplied(), Vec::new());
}

#[test]
fn a_file_named_by_a_file_variable_supplies_its_key_wherever_it_is_mounted() {
    let mut scan = Scan::new();
    scan.scan(
        "database__url",
        "/elsewhere",
        Some("/secrets"),
        &[],
        &[("FIXTURE_DATABASE__URL_FILE", "/elsewhere/database__url")],
    );
    assert_eq!(
        scan.supplied(),
        [("worker", "database.url", vec!["`_FILE` indirection"])]
    );
}

#[test]
fn a_sibling_images_key_is_over_projection_rather_than_an_unknown_name() {
    let mut scan = Scan::new();
    scan.scan("auth__session_ttl", "/secrets", Some("/other"), &[], &[]);
    assert_eq!(scan.surface.unclaimed, Vec::new());
    assert_eq!(scan.surface.over_projected.len(), 1);
    assert_eq!(
        scan.surface.over_projected[0].file_name,
        "auth__session_ttl"
    );
}

#[test]
fn a_name_no_contract_of_the_chart_spells_is_unclaimed() {
    let mut scan = Scan::new();
    scan.scan("nothing__spells_this", "/secrets", Some("/other"), &[], &[]);
    assert_eq!(scan.surface.over_projected, Vec::new());
    assert_eq!(scan.surface.unclaimed.len(), 1);
}

#[test]
fn a_file_gate_three_already_judged_produces_no_second_line() {
    // It is already one line in the check, and a second line saying the same thing in a
    // different report is how a reader learns to skim both.
    let mut scan = Scan::new();
    scan.scan(
        "nothing__spells_this",
        "/secrets",
        Some("/secrets"),
        &[("Deployment app", "app")],
        &[],
    );
    assert_eq!(scan.surface.unclaimed, Vec::new());
    assert_eq!(scan.surface.over_projected, Vec::new());
}

#[test]
fn gate_three_needs_both_halves_before_it_owns_the_verdict() {
    // The declaration lists the container, but the file is outside the secrets directory gate 3
    // resolves, so gate 3 never inspected it and this report must still speak.
    let mut scan = Scan::new();
    scan.scan(
        "nothing__spells_this",
        "/elsewhere",
        Some("/secrets"),
        &[("Deployment app", "app")],
        &[],
    );
    assert_eq!(scan.surface.unclaimed.len(), 1);
}

#[test]
fn a_non_secret_text_key_delivered_as_a_secret_is_recorded_as_policy() {
    let mut scan = Scan::new();
    scan.mine = with_plain_text_key();
    scan.sibling = union("api", API);
    scan.scan("email__username", "/secrets", Some("/secrets"), &[], &[]);
    assert_eq!(
        scan.ledger.elevated.keys().collect::<Vec<_>>(),
        [&(
            "email.username".to_owned(),
            "email__username".to_owned(),
            "text".to_owned()
        )]
    );
    assert_eq!(scan.surface.over_projected, Vec::new());
}

#[test]
fn a_non_secret_key_no_file_can_supply_is_left_to_gate_three() {
    // `worker.concurrency` is an integer, and the mount of it is gate 3's error rather than a
    // policy choice, which it already says at length.
    let mut scan = Scan::new();
    scan.scan(
        "worker__concurrency",
        "/secrets",
        Some("/secrets"),
        &[],
        &[],
    );
    assert!(scan.ledger.elevated.is_empty());
}

// -- which containers the check already opens -------------------------------------------

fn workload() -> Json {
    json!({
        "kind": "Deployment",
        "metadata": {"name": "app", "labels": {"app.kubernetes.io/component": "api"}},
        "spec": {"template": {"spec": {
            "initContainers": [{"name": "migrate"}],
            "containers": [{"name": "api"}],
        }}},
    })
}

fn naming(container: &str) -> Declaration {
    declaration(vec![Document {
        name: "api".to_owned(),
        source: DocumentSource {
            kind: "ConfigMap".to_owned(),
            selector: Vec::new(),
            key: "config.toml".to_owned(),
            format: crate::gate::document::DocumentFormat::Toml,
        },
        images: Vec::new(),
        consumers: Consumers::Declared(vec![Consumer {
            kind: "Deployment".to_owned(),
            selector: vec![("app.kubernetes.io/component".to_owned(), "api".to_owned())],
            name: None,
            containers: vec![container.to_owned()],
        }]),
        exempt: Vec::new(),
        restart: Vec::new(),
        tests: None,
        defaulted: Vec::new(),
    }])
}

#[test]
fn a_declared_container_is_within_reach_and_an_init_container_is_not() {
    // The blind spot this whole report exists for: a container no declaration names is a
    // container gate 3 never opens, and nothing else checks what it mounts.
    let reach = gate_three_reach(&naming("api"), &[workload()]);
    assert_eq!(
        reach.iter().collect::<Vec<_>>(),
        [&("Deployment app".to_owned(), "api".to_owned())]
    );
    assert!(!reach.contains(&("Deployment app".to_owned(), "migrate".to_owned())));
}

// -- the rendering ----------------------------------------------------------------------

#[test]
fn one_container_over_projecting_several_files_is_one_row() {
    // Six near-identical lines is how a reader learns to skim a report.
    let mount = |file_name: &str, values_file: &str| Mount {
        chart: "c".to_owned(),
        values_file: values_file.to_owned(),
        workload: "Deployment app".to_owned(),
        container: "migrate".to_owned(),
        image: "bootstrap".to_owned(),
        mount_path: "/secrets".to_owned(),
        file_name: file_name.to_owned(),
        judged_by_gate_three: false,
    };
    let rows = by_container(&[
        mount("a", "one.yaml"),
        mount("b", "one.yaml"),
        mount("a", "two.yaml"),
    ]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].files, ["a", "b"]);
    assert_eq!(rows[0].values_files, ["one.yaml", "two.yaml"]);
}

#[test]
fn the_dialect_prefix_is_what_every_spelling_shares() {
    assert_eq!(common_prefix(["FIXTURE_A__B", "FIXTURE_C__D"]), "FIXTURE_");
    assert_eq!(common_prefix(["A_ONE", "B_TWO"]), "");
    assert_eq!(common_prefix([]), "");
}
