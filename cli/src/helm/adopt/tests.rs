//! Adopting owed contract keys into a chart that already exists.
//!
//! The scaffold writes a chart nobody has edited, where every rule is about the *output*. This
//! writes into a file somebody else wrote, so half of what can go wrong is about the file rather
//! than about the contract, and none of it is visible from a run over a complete repository —
//! every enrolled chart there binds every key, which is exactly why a key going unbound is worth
//! a command in the first place.
//!
//! Four of the cases are rules a plausible implementation gets wrong: a value that already
//! exists is refused rather than guessed at; a new block lands after the last bound sibling
//! rather than at the end of the file; a key several documents declare becomes one value; and a
//! credential carries no marker. The round trip ties the rest together: what comes out is handed
//! to the bindings gate and the shapes gate — the two that decide whether the chart is complete
//! — rather than compared against bytes written out by hand, which would only prove the
//! generator agrees with itself.
//!
//! Ported case for case from the suite of the script this module replaces, plus the cases for
//! what the port changed: the tie at one line, the diff, and the line endings.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value as Json, json};

use super::{Adoption, Refusal, adopt, splices, write};
use crate::helm::declaration::{Selection, load_declaration, read_yaml};
use crate::helm::{bindings, shapes};
use crate::report::Level;

/// The contract envelope every reader walks — the dialect, the loader variables, `external`,
/// `json_schema` — as the format publishes it, so a case changes only the keys.
const ENVELOPE: &str = include_str!("../../../tests/fixtures/api.json");

/// One contract key, with the fields every consumer of one reads, and `overrides` over them.
fn key(path: &str, overrides: &Json) -> Json {
    let spelt = path.replace('.', "__");
    let mut entry: Map<String, Json> = json!({
        "path": path,
        "env": format!("FIXTURE_{}", spelt.to_uppercase()),
        "env_file": format!("FIXTURE_{}_FILE", spelt.to_uppercase()),
        "secrets_file": spelt,
        "docs": format!("The {path} setting."),
        "ty": "String",
        "text_form": "text",
        "values": [],
        "constraint": {"type": "string"},
        "text_constraint": null,
        "aliases": [],
        "default": null,
        "default_value": null,
        "note": null,
        "required": false,
        "secret": false,
        "reserved": false,
    })
    .as_object()
    .expect("an object")
    .clone();
    for (name, value) in overrides.as_object().expect("overrides are an object") {
        entry.insert(name.clone(), value.clone());
    }
    Json::Object(entry)
}

/// The key every case's chart already binds, so a case is about the keys it adds.
fn session_ttl() -> Json {
    key(
        "auth.session_ttl",
        &json!({"text_form": "integer", "constraint": {"type": "integer"}}),
    )
}

fn integer(path: &str, default: i64) -> Json {
    key(
        path,
        &json!({
            "text_form": "integer",
            "constraint": {"type": "integer"},
            "default_value": default,
        }),
    )
}

/// A contract document carrying these keys.
fn contract_of(keys: &[Json]) -> Json {
    let mut document: Json = serde_json::from_str(ENVELOPE).expect("the fixture is JSON");
    document["schema"]["keys"] = Json::Array(keys.to_vec());
    document
}

const DECLARATION: &str = "\
bindings: true
documents:
  - name: api
    source:
      kind: ConfigMap
      selector: { app: fixture }
      key: config.toml
    images:
      - values: image
        contract: contracts/api.json
";

fn two_documents() -> String {
    format!(
        "{DECLARATION}  - name: worker
    source:
      kind: ConfigMap
      selector: {{ app: fixture-worker }}
      key: config.toml
    images:
      - values: image
        contract: contracts/worker.json
"
    )
}

/// Two documents whose `images[].values` follow the `services.<name>.image` convention — the
/// one a generated root's `$service` guard is read from. [`two_documents`] deliberately does
/// not: its flat `image` path is the ordinary shape, and is what proves a scope that cannot be
/// resolved is printed rather than guessed.
const SCOPED_DOCUMENTS: &str = "\
bindings: true
documents:
  - name: api
    source:
      kind: ConfigMap
      selector: { app.kubernetes.io/component: api }
      key: config.toml
    images:
      - values: services.api.image
        contract: contracts/api.json
  - name: worker
    source:
      kind: ConfigMap
      selector: { app.kubernetes.io/component: worker }
      key: config.toml
    images:
      - values: services.worker.image
        contract: contracts/worker.json
";

const SCOPED_HELPERS: &str = r#"{{- define "fixture.derivedConfig" -}}
{{- $ctx := .ctx -}}
{{- $service := .service -}}
auth:
  {{- with $ctx.Values.auth.sessionTtl }}
  session_ttl: {{ . }}
  {{- end }}
{{- end -}}

{{- define "fixture.secretData" -}}
{{- end -}}
"#;

/// One bound value under a grouping block, then a block with no marker at all — the shape of
/// every chart: the configuration surface first, the chassis after it.
const VALUES: &str = "\
# @schema
# additionalProperties: true
# @schema
# -- The authentication settings.
auth:
  # @schema
  # # @config projection auth.session_ttl
  # type: integer
  # @schema
  # -- How long a session lives.
  sessionTtl: 3600

# @schema
# type: object
# @schema
# -- Requests and limits for the container.
resources: {}
";

/// The shape every contracted chart's helper has: one define per partial, mapping keys at the
/// define's own depth, and `with` blocks under them. Written out rather than generated,
/// because what these cases are about is a *hand-written* template.
const HELPERS: &str = r#"{{- define "fixture.derivedConfig" -}}
auth:
  {{- with .Values.auth.sessionTtl }}
  session_ttl: {{ . }}
  {{- end }}
{{- end -}}

{{- define "fixture.secretData" -}}
{{- end -}}
"#;

/// A throwaway `charts/` tree holding one chart, removed when the case ends.
struct Chart {
    root: PathBuf,
    dir: PathBuf,
}

impl Drop for Chart {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Chart {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "terrace-adopt-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("fixture");
        std::fs::create_dir_all(dir.join("contracts")).expect("a contracts directory");
        std::fs::create_dir_all(dir.join("templates")).expect("a templates directory");
        std::fs::write(
            dir.join("Chart.yaml"),
            "name: fixture\nversion: 1.0.0\nappVersion: v1\n",
        )
        .expect("a chart file");
        let chart = Self { root, dir };
        chart.declaration(DECLARATION);
        chart.values(VALUES);
        chart.template("_helpers.tpl", HELPERS);
        chart
    }

    fn contract(&self, document: &str, keys: &[Json]) -> &Self {
        let vendored = json!({
            "source": {
                "image": "docker.io/example/app",
                "digest": format!("sha256:{}", "1".repeat(64)),
                "sha256": "0".repeat(64),
                "fetched": "2026-01-01T00:00:00Z",
            },
            "contract": contract_of(keys),
        });
        std::fs::write(
            self.dir.join("contracts").join(format!("{document}.json")),
            vendored.to_string(),
        )
        .expect("a vendored contract");
        self
    }

    fn declaration(&self, body: &str) -> &Self {
        std::fs::write(self.dir.join("config-contract.yaml"), body).expect("a declaration");
        self
    }

    fn values(&self, text: &str) -> &Self {
        std::fs::write(self.dir.join("values.yaml"), text).expect("a values file");
        self
    }

    fn template(&self, name: &str, text: &str) -> &Self {
        std::fs::write(self.dir.join("templates").join(name), text).expect("a template");
        self
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.dir.join(relative)).expect("the file is there")
    }

    fn text(&self) -> String {
        self.read("values.yaml")
    }

    fn helper_text(&self) -> String {
        self.read("templates/_helpers.tpl")
    }

    fn declaration_text(&self) -> String {
        self.read("config-contract.yaml")
    }

    fn parsed(&self) -> Json {
        read_yaml(&self.text(), Path::new("values.yaml")).expect("the values still parse")
    }

    fn plan_for(&self, wanted: &[&str]) -> Adoption {
        let wanted: BTreeSet<String> = wanted.iter().map(|path| (*path).to_owned()).collect();
        adopt(&self.dir, &wanted)
            .expect("the chart is readable")
            .expect("the fixture chart is enrolled")
    }

    fn plan(&self) -> Adoption {
        self.plan_for(&[])
    }

    fn write(&self) -> Adoption {
        let planned = self.plan();
        write(&planned).expect("the edits are written");
        planned
    }

    /// The bindings gate over the chart as it now stands, as its error messages.
    fn gate(&self) -> String {
        let found = bindings::check(&self.root, &Selection::all()).expect("the gate runs");
        found
            .report
            .at_level(Level::Error)
            .map(|entry| entry.finding.message.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn declared(&self) -> crate::helm::declaration::Declaration {
        load_declaration(&self.dir)
            .expect("the declaration loads")
            .expect("there is one")
    }
}

fn refusals(planned: &Adoption) -> String {
    planned
        .refusals
        .iter()
        .map(|Refusal { path, reason, .. }| format!("{path} {reason}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn owed_projected(planned: &Adoption) -> Vec<&str> {
    planned
        .owed_projected
        .iter()
        .map(super::Placement::path)
        .collect()
}

/// Every `@schema` block in a values file, keyed by the key it describes, read as helm-schema
/// reads it: the marker is a YAML comment inside the block, so what comes back is the schema.
fn blocks(text: &str) -> BTreeMap<String, Json> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = BTreeMap::new();
    let mut number = 0;
    while number < lines.len() {
        if lines[number].trim() != "# @schema" {
            number += 1;
            continue;
        }
        let start = number + 1;
        let mut end = start;
        while lines[end].trim() != "# @schema" {
            end += 1;
        }
        let body: Vec<&str> = lines[start..end]
            .iter()
            .map(|line| {
                let comment = line.trim();
                comment.strip_prefix('#').unwrap_or(comment).trim()
            })
            .collect();
        let mut value = end + 1;
        while lines[value].trim().starts_with('#') {
            value += 1;
        }
        let name = lines[value].split(':').next().unwrap_or_default().trim();
        let schema = read_yaml(&body.join("\n"), Path::new("block")).expect("a block is YAML");
        found.insert(name.to_owned(), schema);
        number = value;
    }
    found
}

// --------------------------------------------------------------------------------------
// What a new key becomes
// --------------------------------------------------------------------------------------

#[test]
fn a_new_key_becomes_a_value_with_its_marker_type_and_description() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key(
                "log.level",
                &json!({
                    "docs": "Least severe level that is logged.",
                    "constraint": {"type": "string", "enum": ["debug", "info"]},
                    "default_value": "info",
                }),
            ),
        ],
    );
    chart.write();

    assert_eq!(chart.parsed()["log"]["level"], json!("info"));
    assert!(
        chart
            .text()
            .contains("# # @config projection log.level optional")
    );
    assert!(
        chart
            .text()
            .contains("Least severe level that is logged (`log.level`)")
    );
    assert_eq!(
        blocks(&chart.text())["level"],
        json!({"enum": ["debug", "info", null]})
    );
}

#[test]
fn the_default_is_the_one_the_image_publishes() {
    let chart = Chart::new();
    chart.contract("api", &[session_ttl(), integer("worker.concurrency", 8)]);
    chart.write();
    assert_eq!(chart.parsed()["worker"]["concurrency"], json!(8));
}

#[test]
fn two_keys_under_one_new_branch_share_a_grouping_block() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("sentry.dsn_url", &json!({})),
            key("sentry.environment", &json!({})),
        ],
    );
    let planned = chart.write();

    assert_eq!(planned.insertions.len(), 1);
    assert_eq!(chart.text().matches("\nsentry:").count(), 1);
    let sentry = chart.parsed()["sentry"].clone();
    let mut names: Vec<&String> = sentry.as_object().expect("a mapping").keys().collect();
    names.sort();
    assert_eq!(names, ["dsnUrl", "environment"]);
    // The block a contract says nothing about, and the plainest true sentence for it.
    assert!(chart.text().contains("-- The `sentry` settings."));
    assert_eq!(planned.insertions[0].branches, ["sentry"]);
}

#[test]
fn a_credential_gets_a_value_with_no_marker_and_a_write_off() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("database.url", &json!({"secret": true, "required": true})),
        ],
    );
    let planned = chart.write();

    assert_eq!(chart.parsed()["database"]["url"], json!(""));
    assert!(!chart.text().contains("@config projection database.url"));
    assert!(chart.text().contains("`database__url`"));
    let secrets: Vec<&str> = planned
        .plan
        .credentials
        .iter()
        .map(super::Placement::path)
        .collect();
    assert_eq!(secrets, ["database.url"]);
}

#[test]
fn a_reserved_key_gets_no_value_at_all() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("runtime.node_id", &json!({"reserved": true})),
        ],
    );
    let planned = chart.write();

    assert!(chart.parsed().get("runtime").is_none());
    let written_off: Vec<&str> = planned
        .plan
        .written_off
        .iter()
        .map(super::Placement::path)
        .collect();
    assert_eq!(written_off, ["runtime.node_id"]);
    assert!(
        planned.plan.written_off[0]
            .reason
            .contains("Reserved by the loader")
    );
}

// --------------------------------------------------------------------------------------
// Where the block goes
// --------------------------------------------------------------------------------------

fn line_of(text: &str, wanted: &str) -> usize {
    text.lines()
        .position(|line| line == wanted)
        .unwrap_or_else(|| panic!("no line {wanted:?} in\n{text}"))
}

#[test]
fn a_new_top_level_block_lands_after_the_last_bound_block() {
    let chart = Chart::new();
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    chart.write();

    let text = chart.text();
    assert!(line_of(&text, "log:") < line_of(&text, "resources: {}"));
    assert!(line_of(&text, "log:") > line_of(&text, "auth:"));
}

#[test]
fn a_new_leaf_lands_inside_the_block_it_belongs_to() {
    let chart = Chart::new();
    chart.contract("api", &[session_ttl(), key("auth.issuer", &json!({}))]);
    chart.write();

    let auth = chart.parsed()["auth"].clone();
    let mut names: Vec<&String> = auth.as_object().expect("a mapping").keys().collect();
    names.sort();
    assert_eq!(names, ["issuer", "sessionTtl"]);
    let text = chart.text();
    assert!(line_of(&text, "  issuer: null") < line_of(&text, "resources: {}"));
}

#[test]
fn a_leaf_and_a_sibling_block_meeting_at_one_line_both_land_where_they_belong() {
    // `auth` ends on the last line of the bound values, and that same line is where a new
    // top-level block goes. The leaf has to be written first, inside `auth`; the other way
    // round it lands under `log:` and becomes `log.issuer`.
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("auth.issuer", &json!({})),
            key("log.level", &json!({})),
        ],
    );
    let planned = chart.write();

    assert_eq!(planned.insertions.len(), 2);
    assert_eq!(planned.insertions[0].after, planned.insertions[1].after);
    let values = chart.parsed();
    assert_eq!(values["auth"]["issuer"], Json::Null);
    assert!(values["auth"].get("issuer").is_some(), "{values}");
    assert!(values["log"].get("issuer").is_none(), "{values}");
    // Bound, but not yet rendered: `derivedConfig` already writes `auth:`, so where the
    // `issuer` line goes inside that hand-written block is printed for a person rather than
    // written. The gate's template rule is what keeps that from passing silently.
    assert_eq!(
        chart.gate(),
        "binds contract key 'auth.issuer', but no template under fixture/templates/ renders \
             a 'issuer' key. The value exists and is typed, but nothing projects it — \
             indistinguishable, to an operator, from the setting never having been added"
    );
    assert!(!chart.plan().owed());
}

#[test]
fn the_file_keeps_its_line_endings() {
    let chart = Chart::new();
    chart.values(&VALUES.replace('\n', "\r\n"));
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    chart.write();

    let body = chart.text();
    assert_eq!(body.matches('\n').count(), body.matches("\r\n").count());
    assert!(body.contains("log:\r\n"));
}

#[test]
fn a_last_line_with_no_terminator_is_terminated_before_anything_follows_it() {
    let chart = Chart::new();
    chart.values(VALUES.trim_end_matches('\n'));
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    chart.write();
    assert!(chart.text().contains("resources: {}\n"));
    assert_eq!(chart.parsed()["resources"], json!({}));
}

#[test]
fn planning_writes_nothing() {
    let chart = Chart::new();
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    let before = chart.text();
    let planned = chart.plan();

    assert_ne!(planned.insertions, [] as [super::Insertion; 0]);
    assert_eq!(chart.text(), before);
}

// --------------------------------------------------------------------------------------
// What it refuses to write
// --------------------------------------------------------------------------------------

#[test]
fn a_key_whose_value_already_exists_is_refused_and_names_the_marker() {
    let chart = Chart::new();
    // The chart has `auth.sessionTtl`; the contract's key is the one it feeds, and the marker
    // that says so has been lost.
    chart.values(&VALUES.replace("  # # @config projection auth.session_ttl\n", ""));
    chart.contract("api", &[session_ttl()]);
    let planned = chart.write();

    assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
    assert!(refusals(&planned).contains("which this chart already has"));
    assert!(refusals(&planned).contains("# # @config projection auth.session_ttl optional"));
}

#[test]
fn a_leaf_on_the_way_to_the_value_is_refused() {
    let chart = Chart::new();
    // `resources` is a scalar here, so `resources.limit` cannot exist beside it.
    chart.contract("api", &[session_ttl(), key("resources.limit", &json!({}))]);
    chart.values(&VALUES.replace("resources: {}", "resources: none"));
    let planned = chart.write();

    assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
    assert!(refusals(&planned).contains("is a leaf value on the way there"));
}

#[test]
fn documents_that_disagree_about_a_key_are_refused() {
    let chart = Chart::new();
    chart.declaration(&two_documents());
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("log.level", &json!({"constraint": {"type": "string"}})),
        ],
    );
    chart.contract(
        "worker",
        &[key(
            "log.level",
            &json!({"constraint": {"type": "integer"}}),
        )],
    );
    let planned = chart.write();

    assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
    assert!(refusals(&planned).contains("do not agree what it is"));
}

#[test]
fn a_key_bound_in_one_document_and_missing_in_another_is_refused() {
    let chart = Chart::new();
    chart.declaration(&two_documents());
    // The marker is scoped to `api`, so `worker` declares the key and nothing binds it there.
    // A second value under the same name is not the repair; the scope is.
    chart.values(&VALUES.replace(
        "@config projection auth.session_ttl",
        "@config projection api:auth.session_ttl",
    ));
    chart.contract("api", &[session_ttl()]);
    chart.contract("worker", &[session_ttl()]);
    let planned = chart.write();

    assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
    assert!(refusals(&planned).contains("what is owed is that marker's scope"));
}

#[test]
fn a_chart_whose_markers_are_broken_is_not_written_to() {
    let chart = Chart::new();
    chart.values(&format!(
        "{}\n# @schema\n# type: string\n# @schema\n# -- A value.\nspare: \"\"\n",
        VALUES.replace("auth.session_ttl", "auth.session_ttl_typo")
    ));
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    let before = chart.text();
    let planned = chart.plan();

    assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
    assert_ne!(planned.blocked, [] as [String; 0]);
    assert!(planned.blocked.join("\n").contains("auth.session_ttl_typo"));
    write(&planned).expect("writing nothing succeeds");
    assert_eq!(chart.text(), before);
}

// --------------------------------------------------------------------------------------
// One value per key
// --------------------------------------------------------------------------------------

#[test]
fn a_key_several_documents_declare_becomes_one_value() {
    let chart = Chart::new();
    chart.declaration(&two_documents());
    let shared = key("log.level", &json!({"default_value": "info"}));
    chart.contract("api", &[session_ttl(), shared.clone()]);
    chart.contract("worker", &[shared]);
    let planned = chart.write();

    assert_eq!(planned.insertions.len(), 1);
    assert_eq!(
        chart.text().matches("@config projection log.level").count(),
        1
    );
    assert_eq!(chart.parsed()["log"]["level"], json!("info"));
}

// --------------------------------------------------------------------------------------
// A new root only some of a multi-document chart's documents declare
// --------------------------------------------------------------------------------------

#[test]
fn a_key_only_one_document_declares_is_guarded_by_service() {
    let chart = Chart::new();
    chart.declaration(SCOPED_DOCUMENTS);
    chart.template("_helpers.tpl", SCOPED_HELPERS);
    chart.contract("api", &[session_ttl()]);
    chart.contract("worker", &[session_ttl(), integer("queue.workers", 4)]);
    let planned = chart.write();

    assert_eq!(planned.owed_projected, [] as [super::Placement; 0]);
    let body = chart.helper_text();
    assert!(body.contains("{{- if eq $service \"worker\" }}"), "{body}");
    assert!(body.contains("queue:"), "{body}");
    assert!(
        body.contains("workers: {{ $ctx.Values.queue.workers }}"),
        "{body}"
    );
    // The unguarded root stays exactly as it was; the guard wraps only the new one.
    assert!(body.find("auth:") < body.find("{{- if eq $service \"worker\" }}"));
}

#[test]
fn a_new_key_under_a_root_already_guarded_by_service_is_printed_instead() {
    // A root already written, but from inside `{{- if eq $service ... }}` around a
    // `{{- with include ... }}`, not at the define's own depth. A root detector keyed to that
    // depth alone cannot see it, so a new key under the same root looks like a brand-new root
    // and is appended as a second, colliding block — which `fromYaml` resolves by keeping only
    // the second and dropping the first in silence.
    let chart = Chart::new();
    chart.declaration(SCOPED_DOCUMENTS);
    chart.template(
        "_helpers.tpl",
        r#"{{- define "fixture.derivedConfig" -}}
{{- $ctx := .ctx -}}
{{- $service := .service -}}
auth:
  {{- with $ctx.Values.auth.sessionTtl }}
  session_ttl: {{ . }}
  {{- end }}
{{- if eq $service "worker" }}
{{- with include "fixture.queue.config" $ctx }}
queue:
  {{- . | nindent 2 }}
{{- end }}
{{- end }}
{{- end -}}

{{- define "fixture.queue.config" -}}
{{- if not (kindIs "invalid" .Values.queue.workers) }}
workers: {{ .Values.queue.workers }}
{{- end }}
{{- end -}}

{{- define "fixture.secretData" -}}
{{- end -}}
"#,
    );
    chart.contract("api", &[session_ttl()]);
    chart.contract("worker", &[session_ttl(), integer("queue.retries", 3)]);
    let planned = chart.write();

    assert_eq!(owed_projected(&planned), ["queue.retries"]);
    assert_eq!(chart.helper_text().matches("queue:").count(), 1);
}

#[test]
fn new_keys_sharing_a_root_with_different_scopes_are_printed_not_guessed() {
    let chart = Chart::new();
    chart.declaration(SCOPED_DOCUMENTS);
    chart.template("_helpers.tpl", SCOPED_HELPERS);
    chart.contract("api", &[session_ttl(), integer("queue.retries", 3)]);
    chart.contract("worker", &[session_ttl(), integer("queue.workers", 4)]);
    let planned = chart.write();

    let mut owed = owed_projected(&planned);
    owed.sort_unstable();
    assert_eq!(owed, ["queue.retries", "queue.workers"]);
    assert!(!chart.helper_text().contains("queue:"));
}

#[test]
fn a_scoped_key_with_no_resolvable_service_is_printed_not_guessed() {
    let chart = Chart::new();
    chart.declaration(&two_documents());
    chart.contract("api", &[session_ttl(), integer("queue.workers", 4)]);
    chart.contract("worker", &[session_ttl()]);
    let planned = chart.write();

    assert_eq!(owed_projected(&planned), ["queue.workers"]);
    assert!(!chart.helper_text().contains("queue:"));
}

#[test]
fn a_define_that_does_not_wrap_context_is_never_guessed_at() {
    // Included as `(dict "ctx" $ctx "service" $service)`, `.Values` inside the define is a nil
    // pointer. A multi-document chart whose define is not provably shaped that way — no
    // `$ctx := .ctx` line — gets nothing written: bare `.Values` would be as much of a guess
    // as `$ctx.Values` would be wrong.
    let chart = Chart::new();
    chart.declaration(SCOPED_DOCUMENTS);
    chart.template(
        "_helpers.tpl",
        r#"{{- define "fixture.derivedConfig" -}}
{{- $service := .service -}}
auth:
  {{- with .Values.auth.sessionTtl }}
  session_ttl: {{ . }}
  {{- end }}
{{- end -}}

{{- define "fixture.secretData" -}}
{{- end -}}
"#,
    );
    chart.contract("api", &[session_ttl()]);
    chart.contract("worker", &[session_ttl(), integer("queue.workers", 4)]);
    let planned = chart.write();

    assert_eq!(owed_projected(&planned), ["queue.workers"]);
    assert!(!chart.helper_text().contains("queue:"));
}

// --------------------------------------------------------------------------------------
// The declaration
// --------------------------------------------------------------------------------------

#[test]
fn a_credential_is_written_off_and_given_its_value_column() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("database.url", &json!({"secret": true, "required": true})),
        ],
    );
    chart.write();
    let body = chart.declaration_text();

    assert!(body.contains("unbound:"));
    assert!(body.contains("      - database.url"));
    assert!(body.contains("`database__url`"));
    assert!(body.contains("  - key: database.url"));
    assert!(body.contains("    value: database.url"));

    let declared = chart.declared();
    let written_off: Vec<&Vec<String>> = declared.unbound.iter().map(|entry| &entry.keys).collect();
    assert_eq!(written_off, [&vec!["database.url".to_owned()]]);
    let credential = &declared.credentials["database.url"];
    assert_eq!(credential.value.as_deref(), Some("database.url"));
    assert_eq!(credential.note, None);
}

#[test]
fn a_reserved_key_is_written_off_with_the_loader_reason() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("runtime.node_id", &json!({"reserved": true})),
        ],
    );
    chart.write();

    let declared = chart.declared();
    let written_off: Vec<&Vec<String>> = declared.unbound.iter().map(|entry| &entry.keys).collect();
    assert_eq!(written_off, [&vec!["runtime.node_id".to_owned()]]);
    assert!(
        declared.unbound[0]
            .reason
            .contains("Reserved by the loader")
    );
    assert!(declared.credentials.is_empty());
}

#[test]
fn a_credential_the_chart_already_has_a_value_for_is_still_written_off() {
    // The state a run leaves behind if it wrote the value before the declaration. The refusal
    // stands — nothing claims that value feeds this key — but the write-off does not depend on
    // who wrote the value, so its reason drops the clause naming it and the gate is satisfied.
    // Without this, running the command twice leaves a chart it could not finish.
    let chart = Chart::new();
    chart.values(&format!("{VALUES}database:\n  url: \"\"\n"));
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("database.url", &json!({"secret": true, "required": true})),
        ],
    );
    let planned = chart.write();

    assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
    assert_ne!(planned.refusals, [] as [Refusal; 0]);
    let declared = chart.declared();
    let written_off: Vec<&Vec<String>> = declared.unbound.iter().map(|entry| &entry.keys).collect();
    assert_eq!(written_off, [&vec!["database.url".to_owned()]]);
    assert!(
        !declared.unbound[0]
            .reason
            .contains("is the value that carries it")
    );
    // The `value:` column would name a value this command did not write, so it is not claimed.
    assert!(declared.credentials.is_empty());
    assert_eq!(chart.gate(), "");
}

#[test]
fn a_key_already_written_off_is_not_written_off_twice() {
    let chart = Chart::new();
    chart.declaration(&format!(
        "{DECLARATION}unbound:\n  - keys:\n      - database.url\n    reason: Already decided.\n"
    ));
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("database.url", &json!({"secret": true, "required": true})),
        ],
    );
    chart.write();

    let reasons: Vec<String> = chart
        .declared()
        .unbound
        .into_iter()
        .map(|entry| entry.reason)
        .collect();
    assert_eq!(reasons, ["Already decided."]);
}

#[test]
fn a_group_only_partly_written_off_writes_only_its_new_keys() {
    // Every reserved key shares one reason, so two of them form one group. One is already written
    // off; only the other may gain an entry.
    let chart = Chart::new();
    chart.declaration(&format!(
        "{DECLARATION}unbound:\n  - keys:\n      - runtime.node_id\n    reason: Already decided.\n"
    ));
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("runtime.node_id", &json!({"reserved": true})),
            key("runtime.zone", &json!({"reserved": true})),
        ],
    );
    chart.write();

    let declared = chart.declared();
    let written_off: Vec<&Vec<String>> = declared.unbound.iter().map(|entry| &entry.keys).collect();
    assert_eq!(
        written_off,
        [
            &vec!["runtime.node_id".to_owned()],
            &vec!["runtime.zone".to_owned()]
        ]
    );
}

#[test]
fn a_group_appends_to_the_block_a_chart_already_has() {
    let chart = Chart::new();
    chart.declaration(&format!(
        "{DECLARATION}unbound:\n  - keys:\n      - log.level\n    reason: Not surfaced.\n"
    ));
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("log.level", &json!({})),
            key("runtime.node_id", &json!({"reserved": true})),
        ],
    );
    chart.write();

    let written_off: Vec<Vec<String>> = chart
        .declared()
        .unbound
        .into_iter()
        .map(|entry| entry.keys)
        .collect();
    assert_eq!(
        written_off,
        [
            vec!["log.level".to_owned()],
            vec!["runtime.node_id".to_owned()]
        ]
    );
}

#[test]
fn a_credential_row_lands_in_credentials_and_not_in_a_new_unbound_block() {
    // Both new blocks go after the last line of `credentials`: the row has to be written
    // first, so it stays in that block; the other way round it lands under the new `unbound:`
    // as an entry the declaration refuses.
    let chart = Chart::new();
    chart.declaration(&DECLARATION.replace(
        "documents:\n",
        "credentials:\n  - key: other.token\n    value: other.token\n\ndocuments:\n",
    ));
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("other.token", &json!({"secret": true})),
            key("database.url", &json!({"secret": true, "required": true})),
        ],
    );
    chart.values(&format!("{VALUES}other:\n  # -- A token.\n  token: \"\"\n"));
    let planned = chart.write();

    let declaring: Vec<usize> = planned
        .edits
        .iter()
        .filter(|edit| edit.path.ends_with("config-contract.yaml"))
        .map(|edit| edit.after)
        .collect();
    assert_eq!(declaring.len(), 2, "{:?}", planned.edits);
    assert_eq!(
        declaring[0], declaring[1],
        "the two blocks meet at one line"
    );

    let declared = chart.declared();
    assert!(declared.credentials.contains_key("database.url"));
    assert!(declared.credentials.contains_key("other.token"));
    assert!(
        declared
            .unbound
            .iter()
            .any(|entry| entry.keys == ["database.url"])
    );
}

#[test]
fn planning_writes_no_declaration() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("runtime.node_id", &json!({"reserved": true})),
        ],
    );
    let before = chart.declaration_text();
    let planned = chart.plan();

    assert_ne!(planned.edits, [] as [super::FileEdit; 0]);
    assert_eq!(chart.declaration_text(), before);
}

// --------------------------------------------------------------------------------------
// The template lines
// --------------------------------------------------------------------------------------

#[test]
fn a_new_root_is_appended_to_the_derived_config() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("log.level", &json!({"default_value": "info"})),
        ],
    );
    chart.write();
    let body = chart.helper_text();

    assert!(body.contains("log:"));
    assert!(body.contains("{{- with .Values.log.level }}"));
    assert!(body.contains("level: {{ . | quote }}"));
    // Inside the define it belongs to, after everything that was already there.
    assert!(body.find("session_ttl") < body.find("log:"));
    assert!(body.find("log:") < body.find("define \"fixture.secretData\""));
}

#[test]
fn a_root_the_define_already_writes_is_printed_instead() {
    let chart = Chart::new();
    chart.contract("api", &[session_ttl(), key("auth.issuer", &json!({}))]);
    let planned = chart.write();

    assert_eq!(chart.helper_text().matches("auth:").count(), 1);
    assert_eq!(owed_projected(&planned), ["auth.issuer"]);
}

#[test]
fn a_credential_is_appended_to_the_secret_data() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("database.url", &json!({"secret": true, "required": true})),
        ],
    );
    chart.write();
    assert!(
        chart
            .helper_text()
            .contains("database__url: {{ .Values.database.url | quote }}")
    );
}

#[test]
fn a_derived_config_split_into_another_file_is_still_found() {
    // A chart whose services split configuration across several partials keeps
    // `derivedConfig` in `_config.tpl`. Searching `_helpers.tpl` alone would print every
    // projection as still owed and write none, on a chart enrolled like any other.
    let chart = Chart::new();
    chart.template(
        "_helpers.tpl",
        "{{- define \"fixture.secretData\" -}}\n{{- end -}}\n",
    );
    chart.template(
        "_config.tpl",
        "{{- define \"fixture.derivedConfig\" -}}\nauth:\n  {{- with .Values.auth.sessionTtl }}\n  \
             session_ttl: {{ . }}\n  {{- end }}\n{{- end -}}\n",
    );
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("log.level", &json!({"default_value": "info"})),
        ],
    );
    let planned = chart.write();

    assert_eq!(planned.owed_projected, [] as [super::Placement; 0]);
    assert!(chart.read("templates/_config.tpl").contains("log:"));
    assert!(!chart.helper_text().contains("log:"));
}

#[test]
fn a_chart_with_no_helper_prints_every_line() {
    let chart = Chart::new();
    std::fs::remove_file(chart.dir.join("templates").join("_helpers.tpl"))
        .expect("the helper is removed");
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    let planned = chart.write();
    assert_eq!(owed_projected(&planned), ["log.level"]);
}

#[test]
fn a_define_whose_actions_do_not_balance_is_left_alone() {
    // A template shape this cannot read is one it must not edit.
    let chart = Chart::new();
    chart.template(
        "_helpers.tpl",
        "{{- define \"fixture.derivedConfig\" -}}\nauth:\n",
    );
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    let planned = chart.write();

    assert_eq!(owed_projected(&planned), ["log.level"]);
    assert!(!chart.helper_text().contains("log:"));
}

#[test]
fn planning_writes_no_template() {
    let chart = Chart::new();
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    let before = chart.helper_text();
    chart.plan();
    assert_eq!(chart.helper_text(), before);
}

// --------------------------------------------------------------------------------------
// The round trip
// --------------------------------------------------------------------------------------

fn adopted() -> Chart {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key(
                "log.level",
                &json!({"constraint": {"type": "string", "enum": ["debug", "info"]}}),
            ),
            key(
                "sentry.environment",
                &json!({"default_value": "production"}),
            ),
            key(
                "sentry.max_events",
                &json!({"text_form": "integer", "constraint": {"type": "integer"}}),
            ),
        ],
    );
    chart.write();
    chart
}

#[test]
fn the_gate_passes_over_what_was_written() {
    assert_eq!(adopted().gate(), "");
}

#[test]
fn the_shape_gate_passes_over_what_was_written() {
    let chart = adopted();
    let mut read = shapes::Chart::read(&chart.dir).expect("the chart reads");
    shapes::check(&mut read);
    assert!(read.problems.is_empty(), "{:?}", read.problems);
}

#[test]
fn the_values_file_still_parses_and_keeps_what_it_had() {
    let values = adopted().parsed();
    assert_eq!(values["auth"]["sessionTtl"], json!(3600));
    assert_eq!(values["resources"], json!({}));
    let mut names: Vec<&String> = values["sentry"]
        .as_object()
        .expect("a mapping")
        .keys()
        .collect();
    names.sort();
    assert_eq!(names, ["environment", "maxEvents"]);
}

#[test]
fn a_second_run_has_nothing_left_to_do() {
    let again = adopted().plan();
    assert_eq!(again.insertions, [] as [super::Insertion; 0]);
    assert_eq!(again.refusals, [] as [Refusal; 0]);
    assert!(!again.owed());
}

// --------------------------------------------------------------------------------------
// Selecting keys
// --------------------------------------------------------------------------------------

#[test]
fn a_named_key_is_adopted_and_the_rest_are_left() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("log.level", &json!({})),
            key("sentry.environment", &json!({})),
        ],
    );
    let planned = chart.plan_for(&["log.level"]);
    write(&planned).expect("the edits are written");

    assert!(chart.parsed().get("log").is_some());
    assert!(chart.parsed().get("sentry").is_none());
}

#[test]
fn a_key_that_is_not_owed_is_reported_rather_than_ignored() {
    let chart = Chart::new();
    chart.contract("api", &[session_ttl()]);
    let planned = chart.plan_for(&["log.level"]);

    assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
    assert!(refusals(&planned).contains("is not a key fixture is owed a value for"));
}

// --------------------------------------------------------------------------------------
// Enrolment
// --------------------------------------------------------------------------------------

#[test]
fn a_chart_that_is_not_enrolled_is_passed_over() {
    let chart = Chart::new();
    chart.declaration(&DECLARATION.replace("bindings: true", "bindings: false"));
    chart.contract("api", &[key("log.level", &json!({}))]);
    assert!(
        adopt(&chart.dir, &BTreeSet::new())
            .expect("readable")
            .is_none()
    );
}

#[test]
fn a_marker_is_never_written_for_a_chart_with_no_contract() {
    let chart = Chart::new();
    std::fs::remove_file(chart.dir.join("config-contract.yaml")).expect("removed");
    assert!(
        adopt(&chart.dir, &BTreeSet::new())
            .expect("readable")
            .is_none()
    );
}

// --------------------------------------------------------------------------------------
// The diff
// --------------------------------------------------------------------------------------

#[test]
fn the_diff_is_a_unified_diff_of_exactly_what_write_produces() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("auth.issuer", &json!({})),
            key("database.url", &json!({"secret": true})),
        ],
    );
    let planned = chart.plan();
    let found = splices(&planned).expect("the files read");
    assert_eq!(
        found
            .iter()
            .map(|splice| splice.path.file_name().expect("a file name").to_owned())
            .collect::<Vec<_>>(),
        ["values.yaml", "config-contract.yaml", "_helpers.tpl"]
    );

    let values = &found[0];
    let diff = values.diff();
    let shown = super::posix(&values.path);
    assert!(
        diff.starts_with(&format!("--- {shown}\n+++ {shown}\n@@ -")),
        "{diff}"
    );
    // Every line of the diff is a header, a hunk, or one marked line.
    for line in diff.lines().skip(2) {
        assert!(
            line.starts_with("@@ ") || line.starts_with([' ', '+', '-']),
            "{line:?}"
        );
    }
    assert!(diff.contains("\n+  issuer: null\n"), "{diff}");
    assert!(
        !diff
            .lines()
            .any(|line| line.starts_with('-') && !line.starts_with("---"))
    );

    // What the diff describes is what `write` puts on disk.
    write(&planned).expect("written");
    assert_eq!(chart.text(), values.apply());
}

#[test]
fn the_diff_marks_a_last_line_that_had_no_terminator() {
    // The bound block is the whole file and its last line has no terminator, so a new block
    // after it rewrites that line: the old one is removed, saying it had none, and the same
    // text comes back terminated.
    let chart = Chart::new();
    let bound: Vec<&str> = VALUES.lines().take(11).collect();
    chart.values(&bound.join("\n"));
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    let planned = chart.plan();
    assert_eq!(planned.insertions[0].after, 11);

    let found = splices(&planned).expect("the files read");
    let diff = found[0].diff();
    assert!(
        diff.contains("-  sessionTtl: 3600\n\\ No newline at end of file\n+  sessionTtl: 3600\n"),
        "{diff}"
    );
    write(&planned).expect("written");
    assert_eq!(chart.text(), found[0].apply());
    assert_eq!(chart.parsed()["log"]["level"], Json::Null);
}

// --------------------------------------------------------------------------------------
// Shapes an insertion-only writer must not write into
// --------------------------------------------------------------------------------------

#[test]
fn a_block_scalar_that_looks_like_comments_is_not_cut_in_two() {
    // `auth` ends with a `|` scalar whose text has a comment-looking line and a `# @schema`
    // line. Read as a comment and a delimiter, they ended `auth` mid-scalar, and the new
    // top-level block went into the middle of the text.
    let chart = Chart::new();
    chart.values(&VALUES.replace(
        "  sessionTtl: 3600\n",
        "  sessionTtl: 3600\n\n  # -- A banner.\n  banner: |\n    hello\n    # @schema\n    \
             # not a comment\n    world\n",
    ));
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    chart.write();

    let values = chart.parsed();
    assert_eq!(
        values["auth"]["banner"],
        json!("hello\n# @schema\n# not a comment\nworld\n")
    );
    assert_eq!(values["log"]["level"], Json::Null);
    let text = chart.text();
    assert!(line_of(&text, "log:") > line_of(&text, "    world"));
}

#[test]
fn a_parent_written_with_its_value_on_one_line_is_refused() {
    // `telemetry: null` and `limits: {}` are mappings to YAML, but a block below either is a
    // second value for one key, and the file would no longer parse.
    let chart = Chart::new();
    chart.values(&format!(
        "{VALUES}\n# -- Telemetry.\ntelemetry: null\n\n# -- Limits.\nlimits: {{}}\n"
    ));
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("telemetry.level", &json!({})),
            key("limits.burst", &json!({})),
        ],
    );
    let before = chart.text();
    let planned = chart.write();

    assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
    let refused = refusals(&planned);
    assert!(
        refused.contains("telemetry.level needs the chart value 'telemetry.level'"),
        "{refused}"
    );
    assert!(refused.contains("limits.burst"), "{refused}");
    assert!(
        refused.contains("written with its value on the key's own line"),
        "{refused}"
    );
    assert_eq!(chart.text(), before);
    let _ = chart.parsed();
}

#[test]
fn a_declaration_block_written_on_one_line_blocks_the_chart() {
    for (block, owed) in [
        (
            "unbound: []",
            key("runtime.node_id", &json!({"reserved": true})),
        ),
        (
            "credentials: []",
            key("database.url", &json!({"secret": true})),
        ),
    ] {
        let chart = Chart::new();
        chart.declaration(&format!("{DECLARATION}{block}\n"));
        chart.contract("api", &[session_ttl(), owed]);
        let values = chart.text();
        let declaration = chart.declaration_text();
        let planned = chart.write();

        assert!(planned.by_hand, "{block}");
        assert!(
            planned
                .blocked
                .join("\n")
                .contains("is written with its value on the key's own line"),
            "{block}: {:?}",
            planned.blocked
        );
        assert_eq!(planned.insertions, [] as [super::Insertion; 0], "{block}");
        assert_eq!(chart.text(), values, "{block}");
        assert_eq!(chart.declaration_text(), declaration, "{block}");
        let shown = super::report(&planned, false).expect("a report");
        assert!(shown.contains("fixed by hand"), "{shown}");
    }
}

#[test]
fn a_define_closed_on_a_line_shared_with_another_action_is_printed_not_written() {
    // Inserting above `{{- end }}{{- end -}}` lands inside the `with`, under a condition
    // nobody wrote for the new lines.
    let chart = Chart::new();
    chart.template(
            "_helpers.tpl",
            "{{- define \"fixture.derivedConfig\" -}}\nauth:\n  {{- with .Values.auth.sessionTtl }}\n  \
             session_ttl: {{ . }}\n  {{- end }}{{- end -}}\n\n{{- define \"fixture.secretData\" -}}\n\
             {{- end -}}\n",
        );
    let before = chart.helper_text();
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    let planned = chart.write();

    assert_eq!(owed_projected(&planned), ["log.level"]);
    assert_eq!(chart.helper_text(), before);
}

#[test]
fn a_define_opened_and_closed_on_one_line_is_printed_not_written() {
    // Inserting above the closing `end` would put the lines above the define itself.
    let chart = Chart::new();
    chart.template(
        "_helpers.tpl",
        &HELPERS.replace(
            "{{- define \"fixture.secretData\" -}}\n{{- end -}}",
            "{{- define \"fixture.secretData\" -}}{{- end -}}",
        ),
    );
    let before = chart.helper_text();
    chart.contract(
        "api",
        &[session_ttl(), key("database.url", &json!({"secret": true}))],
    );
    let planned = chart.write();

    let owed: Vec<&str> = planned
        .owed_credentials
        .iter()
        .map(super::Placement::path)
        .collect();
    assert_eq!(owed, ["database.url"]);
    assert_eq!(chart.helper_text(), before);
}

#[test]
fn a_key_whose_text_form_this_build_does_not_know_is_refused_and_the_rest_adopted() {
    let chart = Chart::new();
    chart.contract(
        "api",
        &[
            session_ttl(),
            key("log.level", &json!({})),
            key("log.window", &json!({"text_form": "duration"})),
        ],
    );
    let planned = chart.write();

    assert!(
        refusals(&planned)
            .contains("log.window has `text_form` 'duration', which this build does not implement"),
        "{}",
        refusals(&planned)
    );
    assert_eq!(chart.parsed()["log"]["level"], Json::Null);
    assert!(chart.parsed()["log"].get("window").is_none());
}

// --------------------------------------------------------------------------------------
// Writing: all or nothing, and only inside the chart
// --------------------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn a_file_that_resolves_outside_its_chart_fails_the_whole_write_before_anything_lands() {
    // Two charts planned together; the second's helper is a link to a file outside it. The
    // first chart's files are untouched, the outside file is untouched, and no temporary is
    // left behind.
    let first = Chart::new();
    first.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    let second = Chart::new();
    second.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    let outside = second.root.join("outside.tpl");
    std::fs::write(&outside, HELPERS).expect("the outside file");
    let helper = second.dir.join("templates").join("_helpers.tpl");
    std::fs::remove_file(&helper).expect("removed");
    std::os::unix::fs::symlink(&outside, &helper).expect("a symbolic link");

    let before = (first.text(), first.helper_text(), second.text());
    let planned = [first.plan(), second.plan()];
    let error = super::write_all(&planned).expect_err("the link escapes its chart");
    assert!(error.to_string().contains("outside the chart"), "{error}");

    assert_eq!((first.text(), first.helper_text(), second.text()), before);
    assert_eq!(std::fs::read_to_string(&outside).expect("outside"), HELPERS);
    for chart in [&first, &second] {
        for directory in [chart.dir.clone(), chart.dir.join("templates")] {
            for entry in std::fs::read_dir(&directory).expect("a directory") {
                let name = entry.expect("an entry").file_name();
                assert!(
                    !name.to_string_lossy().ends_with(".tmp"),
                    "a temporary was left: {name:?}"
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn a_write_keeps_each_files_permissions() {
    use std::os::unix::fs::PermissionsExt as _;

    let chart = Chart::new();
    let values = chart.dir.join("values.yaml");
    std::fs::set_permissions(&values, std::fs::Permissions::from_mode(0o640))
        .expect("permissions set");
    chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
    chart.write();
    let mode = std::fs::metadata(&values)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o640);
    assert!(chart.text().contains("log:"));
}
