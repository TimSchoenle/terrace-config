//! Gates 2 and 3, over hand-built containers rather than over a render.
//!
//! Ported from the suite that held the implementation this was taken from, and kept in its shape
//! because the shape was already right: each gate is a function of manifests and a contract
//! returning findings, so a test constructs the container it wants and reads the list back — no
//! chart, no render, no cluster.
//!
//! What this proves is that **the rule that fires is the one that should**, and that a rule which
//! should stay quiet does. That the gates fire against a real tree at all is
//! [`parity`](../parity.rs)'s job, over a corpus.

#![cfg(feature = "helm")]

use std::path::{Path, PathBuf};

use serde_json::{Value as Json, json};
use terrace_contract::gate::{Relaxed, ServiceLinks, check_container};
use terrace_contract::report::{Finding, Level};
use terrace_contract::union::{Union, union_contracts};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// The contracts of one or more fixture images, merged.
fn union(names: &[&str]) -> Union {
    let loaded: Vec<(String, Json)> = names
        .iter()
        .map(|name| {
            let path = fixtures().join(format!("{name}.json"));
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()));
            (
                (*name).to_owned(),
                serde_json::from_str(&text).expect("a fixture is JSON"),
            )
        })
        .collect();
    union_contracts(&loaded).expect("the fixtures merge")
}

/// A container running the one image every fixture describes.
fn container(env: &[(&str, &str)], mounts: &[Json]) -> Json {
    json!({
        "name": "app",
        "image": format!("example/app:v1@sha256:{}", "0".repeat(64)),
        "env": env.iter().map(|(name, value)| json!({"name": name, "value": value}))
            .collect::<Vec<_>>(),
        "volumeMounts": mounts,
    })
}

fn check(env: &[(&str, &str)], merged: &Union) -> Vec<Finding> {
    check_container(
        &[],
        &json!({}),
        &container(env, &[]),
        merged,
        Relaxed::default(),
    )
    .expect("the fixture is readable")
}

fn messages(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|finding| finding.message.as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

// ---------------------------------------------------------------------------------------------
// Gate 2 — the environment
// ---------------------------------------------------------------------------------------------

#[test]
fn a_correct_environment_is_silent() {
    let merged = union(&["api"]);
    assert_eq!(
        check(
            &[("FIXTURE_AUTH__SESSION_TTL", "60"), ("PORT", "8080")],
            &merged
        ),
        Vec::new()
    );
}

#[test]
fn a_prefixed_variable_spelling_nothing_is_reported() {
    let merged = union(&["api"]);
    let findings = check(&[("FIXTURE_NOPE", "1")], &merged);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].message.contains("FIXTURE_NOPE"), "{findings:?}");
}

#[test]
fn a_near_miss_is_offered_a_suggestion() {
    let merged = union(&["api"]);
    let findings = check(&[("FIXTURE_AUTH__SESSION_TTLS", "60")], &merged);
    assert!(messages(&findings).contains("did you mean"), "{findings:?}");
}

#[test]
fn a_value_of_the_wrong_form_is_reported() {
    let merged = union(&["api"]);
    let findings = check(&[("PORT", "http")], &merged);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].message.contains("http"), "{findings:?}");
}

#[test]
fn a_value_outside_its_bound_is_reported() {
    // The one only the range step reaches: `99999` is a well-formed integer, so no pattern says
    // anything about it, and only the `maximum` catches it not fitting a `u16`.
    let merged = union(&["api"]);
    let findings = check(&[("PORT", "99999")], &merged);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].message.contains("65535"), "{findings:?}");
}

#[test]
fn one_value_produces_one_line() {
    // "not an integer" followed by "not below 65535" says nothing the first did not, and the
    // second would be reporting on a read that never happened.
    let merged = union(&["api"]);
    assert_eq!(check(&[("PORT", "http")], &merged).len(), 1);
}

#[test]
fn a_structured_key_without_brackets_is_reported() {
    let merged = union(&["api"]);
    let findings = check(&[("FIXTURE_GITHUB__REPOS", "a,b")], &merged);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].message.contains("brackets"), "{findings:?}");
}

#[test]
fn a_structured_key_with_brackets_is_silent() {
    let merged = union(&["api"]);
    assert_eq!(
        check(&[("FIXTURE_GITHUB__REPOS", r#"["a","b"]"#)], &merged),
        Vec::new()
    );
}

#[test]
fn an_ignored_variable_is_silent() {
    let merged = union(&["api"]);
    assert_eq!(
        check(&[("KUBERNETES_SERVICE_HOST", "10.0.0.1")], &merged),
        Vec::new()
    );
}

#[test]
fn an_unaccounted_variable_follows_the_documents_own_policy() {
    let strict = union(&["api"]);
    assert_eq!(check(&[("NOVEL", "1")], &strict)[0].level, Level::Error);

    // `worker`'s `external.unknown` is `warn`.
    let permissive = union(&["worker"]);
    assert_eq!(
        check(&[("NOVEL", "1")], &permissive)[0].level,
        Level::Warning
    );
}

#[test]
fn a_value_from_entry_is_classified_but_not_value_checked() {
    let merged = union(&["api"]);
    let held = json!({
        "name": "app",
        "env": [{"name": "PORT", "valueFrom": {"fieldRef": {"fieldPath": "x"}}}],
    });
    let findings = check_container(&[], &json!({}), &held, &merged, Relaxed::default())
        .expect("the fixture is readable");
    assert_eq!(findings, Vec::new());
}

#[test]
fn an_exemption_relaxes_the_gate() {
    let merged = union(&["api"]);
    let relaxed = Relaxed {
        env: true,
        ..Relaxed::default()
    };
    let findings = check_container(
        &[],
        &json!({}),
        &container(&[("FIXTURE_NOPE", "1")], &[]),
        &merged,
        relaxed,
    )
    .expect("the fixture is readable");
    assert_eq!(findings, Vec::new());
}

#[test]
fn the_union_would_hide_what_one_images_contract_catches() {
    // The whole reason the scopes are two rather than one: a variable only a sibling image reads
    // is precisely the defect gate 2 exists to catch, and against the merged contract it is a
    // perfectly ordinary key.
    let merged = union(&["api", "worker"]);
    let alone = union(&["api"]);
    let env = [("FIXTURE_WORKER__CONCURRENCY", "4")];

    assert_eq!(check(&env, &merged), Vec::new());
    let against_own = check(&env, &alone);
    assert_eq!(against_own.len(), 1, "{against_own:?}");
    assert!(
        against_own[0]
            .message
            .contains("FIXTURE_WORKER__CONCURRENCY"),
        "{against_own:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Gate 3 — the files
// ---------------------------------------------------------------------------------------------

/// One projected secret volume presenting the named files.
fn secret_volume(items: &[&str]) -> Json {
    json!({
        "name": "secrets",
        "projected": {"sources": [{"secret": {
            "name": "app",
            "items": items.iter().map(|name| json!({"key": name, "path": name}))
                .collect::<Vec<_>>(),
        }}]},
    })
}

fn run_files(
    env: &[(&str, &str)],
    mounts: &[Json],
    volumes: &[Json],
    manifests: &[Json],
) -> Vec<Finding> {
    let merged = union(&["api"]);
    let spec = json!({"volumes": volumes});
    check_container(
        manifests,
        &spec,
        &container(env, mounts),
        &merged,
        Relaxed::default(),
    )
    .expect("the fixture is readable")
}

fn mount(path: &str) -> Json {
    json!({"name": "secrets", "mountPath": path})
}

#[test]
fn a_correctly_named_secret_file_is_silent() {
    let findings = run_files(
        &[("FIXTURE_SECRETS_DIR", "/secrets")],
        &[mount("/secrets")],
        &[secret_volume(&["database__url"])],
        &[],
    );
    assert_eq!(findings, Vec::new());
}

#[test]
fn a_secret_file_spelling_no_key_is_reported() {
    let findings = run_files(
        &[("FIXTURE_SECRETS_DIR", "/secrets")],
        &[mount("/secrets")],
        &[secret_volume(&["database__nope"])],
        &[],
    );
    assert!(
        messages(&findings).contains("database__nope"),
        "{findings:?}"
    );
    assert!(
        messages(&findings).contains("spells no key"),
        "{findings:?}"
    );
}

#[test]
fn a_secret_file_for_a_key_that_is_not_text_is_reported() {
    let findings = run_files(
        &[("FIXTURE_SECRETS_DIR", "/secrets")],
        &[mount("/secrets")],
        &[secret_volume(&["auth__session_ttl"])],
        &[],
    );
    assert!(
        messages(&findings).contains("cannot be supplied by a file"),
        "{findings:?}"
    );
}

#[test]
fn key_named_files_mounted_where_nothing_reads_them_are_reported() {
    // The worse half of the defect: the files exist, the loader never looks, and every credential
    // falls back to a default. Nothing renders wrong and nothing fails to start.
    let findings = run_files(
        &[],
        &[mount("/elsewhere")],
        &[secret_volume(&["database__url"])],
        &[],
    );
    assert!(
        messages(&findings).contains("nothing reads it"),
        "{findings:?}"
    );
}

#[test]
fn a_file_read_by_indirection_is_not_reported() {
    let findings = run_files(
        &[("FIXTURE_DATABASE__URL_FILE", "/elsewhere/database__url")],
        &[mount("/elsewhere")],
        &[secret_volume(&["database__url"])],
        &[],
    );
    assert_eq!(findings, Vec::new());
}

#[test]
fn an_indirection_variable_pointing_outside_every_mount_is_reported() {
    let findings = run_files(
        &[("FIXTURE_DATABASE__URL_FILE", "/nowhere/x")],
        &[],
        &[],
        &[],
    );
    assert!(
        messages(&findings).contains("not inside any volume"),
        "{findings:?}"
    );
}

#[test]
fn an_indirection_variable_for_a_key_that_is_not_text_is_reported() {
    let findings = run_files(
        &[("FIXTURE_AUTH__SESSION_TTL_FILE", "/secrets/x")],
        &[mount("/secrets")],
        &[],
        &[],
    );
    assert!(
        messages(&findings).contains("cannot be supplied by a file"),
        "{findings:?}"
    );
}

#[test]
fn a_loader_path_pointing_outside_every_mount_is_reported() {
    let findings = run_files(&[("FIXTURE_CONFIG", "/etc/app")], &[], &[], &[]);
    assert!(
        messages(&findings).contains("not inside any volume"),
        "{findings:?}"
    );
}

#[test]
fn a_secrets_dir_below_a_mount_point_warns_that_it_could_not_look() {
    let findings = run_files(
        &[("FIXTURE_SECRETS_DIR", "/mnt/sub")],
        &[mount("/mnt")],
        &[secret_volume(&["database__url"])],
        &[],
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.level == Level::Warning),
        "{findings:?}"
    );
}

#[test]
fn a_source_without_items_is_followed_back_to_the_rendered_secret() {
    // The existing-Secret case, which would otherwise go unchecked entirely.
    let findings = run_files(
        &[("FIXTURE_SECRETS_DIR", "/secrets")],
        &[mount("/secrets")],
        &[json!({"name": "secrets", "projected": {"sources": [{"secret": {"name": "app"}}]}})],
        &[json!({
            "kind": "Secret",
            "metadata": {"name": "app"},
            "data": {"database__nope": "eA=="},
        })],
    );
    assert!(
        messages(&findings).contains("database__nope"),
        "{findings:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// The layer collision, and the precondition
// ---------------------------------------------------------------------------------------------

#[test]
fn one_key_from_two_layers_is_reported() {
    // The pair a loader refuses at boot, which nothing else in a chart repository can see —
    // because seeing it requires knowing that `FIXTURE_DATABASE__URL` and the file `database__url`
    // are the same key.
    let findings = run_files(
        &[
            ("FIXTURE_SECRETS_DIR", "/secrets"),
            ("FIXTURE_DATABASE__URL", "postgres://x"),
        ],
        &[mount("/secrets")],
        &[secret_volume(&["database__url"])],
        &[json!({
            "kind": "Secret",
            "metadata": {"name": "app"},
            "data": {"database__url": "eA=="},
        })],
    );
    assert!(messages(&findings).contains("supplied by"), "{findings:?}");
    assert!(messages(&findings).contains("database.url"), "{findings:?}");
}

#[test]
fn one_key_from_one_layer_is_silent() {
    let findings = run_files(&[("FIXTURE_DATABASE__URL", "postgres://x")], &[], &[], &[]);
    assert_eq!(findings, Vec::new());
}

#[test]
fn a_pod_without_the_service_links_switch_is_reported() {
    let merged = union(&["api"]);
    let findings = ServiceLinks::check(&json!({}), &merged);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].message.contains("enableServiceLinks"),
        "{findings:?}"
    );
}

#[test]
fn a_pod_with_the_switch_is_silent() {
    let merged = union(&["api"]);
    assert_eq!(
        ServiceLinks::check(&json!({"enableServiceLinks": false}), &merged),
        Vec::new()
    );
}

#[test]
fn setting_it_true_is_not_setting_it() {
    let merged = union(&["api"]);
    assert_eq!(
        ServiceLinks::check(&json!({"enableServiceLinks": true}), &merged).len(),
        1
    );
}
