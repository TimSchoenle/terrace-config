use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Map, Value as Json, json};

use super::{
    Reader, Setting, Surface, collect, compact, describe_constraint, full, reader_list,
    report_divergences, select,
};
use crate::helm::declaration::load_declaration;
use crate::report::Report;

const API: &str = include_str!("../../../tests/fixtures/api.json");
const WORKER: &str = include_str!("../../../tests/fixtures/worker.json");

const API_DIGEST: &str = "sha256:abababababababababababababababababababababababababababababababab";
const WORKER_DIGEST: &str =
    "sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
const OTHER_DIGEST: &str =
    "sha256:efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef";

fn fixture(name: &str) -> Json {
    serde_json::from_str(match name {
        "api" => API,
        _ => WORKER,
    })
    .expect("a fixture contract")
}

fn setting(name: &str, occurrences: &[(&str, Json)]) -> Setting {
    Setting {
        name: name.to_owned(),
        occurrences: occurrences
            .iter()
            .map(|(reader, entry)| {
                (
                    (*reader).to_owned(),
                    entry.as_object().expect("an object").clone(),
                )
            })
            .collect(),
    }
}

/// A throwaway chart pinning one image per contract, wired exactly as a real one is.
struct Chart(PathBuf);

impl Chart {
    /// `images` is `(document name, fixture name, digest)`, one document per image.
    fn of(images: &[(&str, &str, &str)]) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let at = std::env::temp_dir()
            .join(format!(
                "terrace-explain-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ))
            .join("demo");
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(at.join("contracts")).expect("a chart directory");

        let mut values = Map::new();
        let mut declared = String::new();
        for (name, contract, digest) in images {
            use std::fmt::Write as _;

            std::fs::write(
                at.join("contracts").join(format!("{name}.json")),
                serde_json::to_string(&json!({
                    "source": {
                        "image": format!("docker.io/demo/{name}"),
                        "digest": digest,
                        "sha256": "0".repeat(64),
                        "fetched": "2026-01-01T00:00:00Z",
                    },
                    "contract": fixture(contract),
                }))
                .expect("it serialises"),
            )
            .expect("a vendored contract is written");
            values.insert(
                (*name).to_owned(),
                json!({"repository": format!("demo/{name}"), "tag": format!("v1@{digest}")}),
            );
            let _ = write!(
                declared,
                "  - name: {name}\n    source: {{ kind: ConfigMap, key: config.toml }}\n    \
                     images:\n      - values: {name}\n        contract: contracts/{name}.json\n"
            );
        }

        std::fs::write(at.join("Chart.yaml"), "name: demo\nappVersion: v1\n")
            .expect("a chart file");
        std::fs::write(
            at.join("values.yaml"),
            serde_json::to_string(&Json::Object(values)).expect("it serialises"),
        )
        .expect("a values file");
        std::fs::write(
            at.join("config-contract.yaml"),
            format!("documents:\n{declared}"),
        )
        .expect("a declaration");
        Self(at)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }

    /// Repin one image at another digest, which is what the interlock exists to catch.
    fn repin(&self, name: &str, digest: &str) {
        let text =
            std::fs::read_to_string(self.0.join("values.yaml")).expect("the values are there");
        let mut values: Json = serde_json::from_str(&text).expect("they parse");
        values[name]["tag"] = json!(format!("v2@{digest}"));
        std::fs::write(
            self.0.join("values.yaml"),
            serde_json::to_string(&values).expect("it serialises"),
        )
        .expect("the values are written");
    }

    fn collected(&self) -> (Option<Surface>, Report) {
        let mut report = Report::new();
        let declaration = load_declaration(self.path())
            .expect("the declaration reads")
            .expect("it is there");
        let surface = collect(self.path(), &declaration, &mut report).expect("it collects");
        (surface, report)
    }
}

impl Drop for Chart {
    fn drop(&mut self) {
        if let Some(parent) = self.0.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}

// -- which images read which setting -----------------------------------------------------

fn two_images() -> Chart {
    Chart::of(&[
        ("api", "api", API_DIGEST),
        ("worker", "worker", WORKER_DIGEST),
    ])
}

#[test]
fn a_setting_names_exactly_the_images_that_read_it() {
    // The one fact this produces that exists nowhere else: not in the chart, not in the
    // declaration, not in the README.
    let chart = two_images();
    let (surface, _) = chart.collected();
    let surface = surface.expect("the contracts bind");
    assert_eq!(surface.keys["auth.session_ttl"].readers(), ["api"]);
    assert_eq!(surface.keys["worker.concurrency"].readers(), ["worker"]);
    assert_eq!(surface.keys["database.url"].readers(), ["api", "worker"]);
    assert_eq!(
        surface.keys.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "auth.session_ttl",
            "database.url",
            "github.repos",
            "log.level",
            "tuning.ratio",
            "worker.concurrency",
            "worker.debug",
        ]
    );
}

#[test]
fn the_images_are_named_by_their_contract_and_carry_their_provenance() {
    let chart = two_images();
    let (surface, _) = chart.collected();
    let surface = surface.expect("the contracts bind");
    let api = surface
        .readers
        .iter()
        .find(|reader| reader.name == "api")
        .expect("the api image");
    assert_eq!(api.image, "docker.io/demo/api");
    assert_eq!(api.digest, API_DIGEST);
    assert_eq!(api.documents, ["api"]);
}

#[test]
fn readers_are_sorted_rather_than_declaration_ordered() {
    // The order every reader list and every "the images disagree" side is printed in is a
    // property of the contracts rather than of where a maintainer added a document.
    let chart = Chart::of(&[
        ("worker", "worker", WORKER_DIGEST),
        ("api", "api", API_DIGEST),
    ]);
    let (surface, _) = chart.collected();
    assert_eq!(
        surface.expect("the contracts bind").keys["database.url"].readers(),
        ["api", "worker"]
    );
}

#[test]
fn all_of_them_is_said_as_all_rather_than_listed() {
    // The names are in the header; what is worth reading here is that the setting is
    // fleet-wide. One image is not a fleet, and `all 1` would be a strange way to say a name.
    let chart = two_images();
    let (surface, _) = chart.collected();
    let surface = surface.expect("the contracts bind");
    let shared = &surface.keys["database.url"];
    assert_eq!(reader_list(shared, 2), "all 2");
    assert_eq!(reader_list(shared, 3), "api, worker");
    assert_eq!(reader_list(&surface.keys["log.level"], 1), "api");
}

// -- merging what several images say -----------------------------------------------------

#[test]
fn required_unions_because_any_reader_requiring_it_wins() {
    let merged = setting(
        "database.url",
        &[
            ("api", json!({"path": "database.url", "required": false})),
            ("worker", json!({"path": "database.url", "required": true})),
        ],
    );
    assert_eq!(merged.value("required"), Some(&json!(true)));
    // The rule restated rather than reinvented, so a union is not reported as a disagreement.
    assert!(!merged.divergent().iter().any(|name| name == "required"));
}

#[test]
fn empty_prose_is_an_absence_rather_than_a_contradiction() {
    // An image that says nothing about a key it shares has not contradicted the one that does.
    let merged = setting(
        "bind_addr",
        &[
            ("api", json!({"path": "bind_addr", "docs": ""})),
            (
                "worker",
                json!({"path": "bind_addr", "docs": "The listener."}),
            ),
        ],
    );
    assert_eq!(merged.value("docs"), Some(&json!("The listener.")));
}

#[test]
fn two_descriptions_of_one_setting_are_both_kept() {
    let merged = setting(
        "bind_addr",
        &[
            ("api", json!({"path": "bind_addr", "docs": "Public."})),
            ("render", json!({"path": "bind_addr", "docs": ""})),
            ("worker", json!({"path": "bind_addr", "docs": "Ops only."})),
        ],
    );
    assert_eq!(merged.divergent(), ["docs"]);
    assert_eq!(
        merged.variants("docs"),
        [
            (json!("Public."), vec!["api"]),
            (json!(""), vec!["render"]),
            (json!("Ops only."), vec!["worker"]),
        ]
    );
}

#[test]
fn a_disagreement_about_a_constraint_is_reported_and_not_resolved() {
    let merged = setting(
        "port",
        &[
            (
                "api",
                json!({"path": "port", "constraint": {"type": "integer", "maximum": 65535}}),
            ),
            (
                "worker",
                json!({"path": "port", "constraint": {"type": "integer"}}),
            ),
        ],
    );
    assert_eq!(merged.divergent(), ["constraint"]);
    assert_eq!(
        merged.representative()["constraint"]["maximum"],
        json!(65535)
    );
}

#[test]
fn a_field_only_one_image_publishes_is_still_carried() {
    let merged = setting(
        "log.level",
        &[
            ("api", json!({"path": "log.level"})),
            (
                "worker",
                json!({"path": "log.level", "note": "ignored in tests"}),
            ),
        ],
    );
    assert_eq!(merged.value("note"), Some(&json!("ignored in tests")));
}

#[test]
fn the_representative_carries_every_field_any_image_published() {
    // Including one only the second image says anything about, and in the document model's own
    // order, which is the order this build's producer writes a contract in, so the JSON diffs
    // cleanly against the file it came from.
    let merged = setting(
        "log.level",
        &[
            (
                "api",
                json!({"path": "log.level", "env": "X", "ty": "Level"}),
            ),
            (
                "worker",
                json!({"path": "log.level", "env": "X", "ty": "Level", "secret": false}),
            ),
        ],
    );
    assert_eq!(
        merged
            .representative()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["env", "path", "secret", "ty"]
    );
}

// -- the divergence nothing else can see -------------------------------------------------

#[test]
fn a_shared_setting_described_differently_becomes_a_warning() {
    // Each document is validated against its own images' union, so two images that never share
    // a document never have their descriptions of a shared setting compared.
    let chart = two_images();
    let (surface, mut report) = chart.collected();
    let mut surface = surface.expect("the contracts bind");
    surface
        .keys
        .get_mut("database.url")
        .expect("the key")
        .occurrences[1]
        .1
        .insert("docs".to_owned(), json!("Somewhere else entirely."));

    report_divergences(&surface, None, &mut report);
    let messages: Vec<&str> = report
        .entries()
        .iter()
        .map(|entry| entry.finding.message.as_str())
        .collect();
    assert!(
        messages.iter().any(|held| held.contains("database.url")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|held| held.contains("`docs`")),
        "{messages:?}"
    );
    assert_eq!(report.error_count(), 0);
}

#[test]
fn a_divergence_outside_the_selection_is_not_reported() {
    // A warning about a setting the reader did not ask about is noise on every run, and would
    // train them to ignore the one that matters.
    let chart = two_images();
    let (surface, mut report) = chart.collected();
    let mut surface = surface.expect("the contracts bind");
    surface
        .keys
        .get_mut("database.url")
        .expect("the key")
        .occurrences[1]
        .1
        .insert("docs".to_owned(), json!("drifted"));
    report_divergences(&surface, Some("log.level"), &mut report);
    assert_eq!(report.entries().len(), 0);
}

// -- the interlock -----------------------------------------------------------------------

#[test]
fn a_bumped_digest_produces_no_surface_at_all() {
    // Printing a contract that is not for the pinned digest is a confident wrong answer to the
    // only question anyone runs this to ask.
    let chart = Chart::of(&[("api", "api", API_DIGEST)]);
    chart.repin("api", OTHER_DIGEST);
    let (surface, report) = chart.collected();
    assert!(surface.is_none());
    assert!(report.error_count() > 0);
}

#[test]
fn one_stale_document_refuses_the_whole_chart() {
    // Rather than printing the one that bound and quietly dropping the other: a listing missing
    // a document is indistinguishable from an image that reads nothing.
    let chart = two_images();
    chart.repin("worker", OTHER_DIGEST);
    assert!(chart.collected().0.is_none());
}

// -- selecting ---------------------------------------------------------------------------

fn settings() -> BTreeMap<String, Setting> {
    [
        "auth.session_ttl",
        "database.url",
        "log.level",
        "Log.Rotation",
    ]
    .into_iter()
    .map(|name| {
        (
            name.to_owned(),
            setting(name, &[("api", json!({"path": name}))]),
        )
    })
    .collect()
}

fn selected(pattern: Option<&str>) -> Vec<String> {
    select(&settings(), pattern)
        .into_iter()
        .map(|item| item.name.clone())
        .collect()
}

#[test]
fn no_pattern_takes_everything_in_path_order() {
    assert_eq!(
        selected(None),
        [
            "Log.Rotation",
            "auth.session_ttl",
            "database.url",
            "log.level"
        ]
    );
}

#[test]
fn a_plain_pattern_is_a_case_insensitive_substring() {
    // The name an operator has in hand is usually a fragment of a path they read somewhere.
    assert_eq!(selected(Some("LOG")), ["Log.Rotation", "log.level"]);
}

#[test]
fn a_glob_is_matched_against_the_whole_path() {
    assert_eq!(selected(Some("log.*")), ["log.level"]);
    assert_eq!(selected(Some("auth.*.ttl")), Vec::<String>::new());
    assert_eq!(selected(Some("log.leve?")), ["log.level"]);
    assert_eq!(selected(Some("[dl]og.level")), ["log.level"]);
    assert_eq!(selected(Some("[!L]og.level")), ["log.level"]);
}

// -- rendering a constraint --------------------------------------------------------------

fn described(constraint: &Json) -> String {
    describe_constraint(Some(constraint))
}

#[test]
fn a_bound_reads_as_a_range_and_says_which_side_when_it_is_one_sided() {
    assert_eq!(
        described(&json!({"type": "integer", "minimum": 0, "maximum": 65535})),
        "integer, 0 to 65535"
    );
    assert_eq!(
        described(&json!({"type": "integer", "minimum": 1})),
        "integer, at least 1"
    );
    assert_eq!(
        described(&json!({"type": "integer", "maximum": 9})),
        "integer, at most 9"
    );
    assert_eq!(described(&json!({"exclusiveMinimum": 0})), "above 0");
}

#[test]
fn a_vocabulary_this_renderer_knows_reads_as_prose() {
    assert_eq!(
        described(&json!({"type": "string", "enum": ["a", "b"]})),
        "string, one of \"a\" | \"b\""
    );
    assert_eq!(
        described(&json!({"type": "string", "minLength": 1, "maxLength": 64})),
        "string, 1 to 64 characters"
    );
    assert_eq!(
        described(&json!({"type": "array", "items": {"type": "string"}, "uniqueItems": true})),
        "array, distinct, of string"
    );
}

#[test]
fn an_unrecognised_keyword_is_printed_rather_than_dropped() {
    // Under-reporting a constraint is the failure this pipeline exists to remove, and it is no
    // better coming from the explainer than from a gate.
    assert!(
        described(&json!({"type": "string", "contentEncoding": "base64"}))
            .contains("contentEncoding=\"base64\""),
        "{}",
        described(&json!({"type": "string", "contentEncoding": "base64"}))
    );
}

#[test]
fn a_structs_conditions_are_said_by_their_sentences() {
    assert_eq!(
        described(&json!({
            "type": "object",
            "properties": {"url": {"type": "string"}},
            "allOf": [
                {"description": "`url` is set", "required": ["url"]},
                {"anyOf": [{"required": ["url"]}]},
            ],
        })),
        r#"object, fields url, holds: `url` is set, allOf=[{"anyOf":[{"required":["url"]}]}]"#
    );
}

#[test]
fn a_maps_entry_name_pattern_is_said_and_anything_else_under_it_is_printed() {
    assert_eq!(
        described(&json!({
            "type": "object",
            "additionalProperties": {"type": "string"},
            "required": ["home"],
            "propertyNames": {"pattern": "^[a-z]+$"},
        })),
        "object, of string, must contain: home, entry names matching ^[a-z]+$"
    );
    assert_eq!(
        described(&json!({
            "type": "object",
            "propertyNames": {"maxLength": 3},
        })),
        r#"object, propertyNames={"maxLength":3}"#
    );
}

#[test]
fn a_maps_required_entries_are_said_and_a_structs_stay_with_its_fields() {
    assert_eq!(
        described(&json!({
            "type": "object",
            "additionalProperties": {"type": "string"},
            "required": ["imprint", "privacy"],
        })),
        "object, of string, must contain: imprint, privacy"
    );
    assert!(
        !described(&json!({
            "type": "object",
            "properties": {"id": {"type": "string"}},
            "required": ["id"],
        }))
        .contains("must contain"),
    );
}

#[test]
fn nothing_to_say_is_said_with_nothing() {
    assert_eq!(describe_constraint(None), "");
    assert_eq!(described(&json!({})), "");
}

// -- rendering an entry ------------------------------------------------------------------

fn dialect() -> Map<String, Json> {
    json!({
        "prefix": "FIXTURE_",
        "nesting_separator": "__",
        "indirection_suffix": "_FILE",
    })
    .as_object()
    .expect("an object")
    .clone()
}

fn key(path: &str) -> Setting {
    let contract = fixture("api");
    let entry = contract["schema"]["keys"]
        .as_array()
        .expect("a list of keys")
        .iter()
        .find(|held| held["path"] == json!(path))
        .unwrap_or_else(|| panic!("{path}"));
    setting(path, &[("api", entry.clone())])
}

fn rendered(path: &str, total: usize) -> String {
    full(&key(path), &[], &dialect(), total).join("\n")
}

fn reader(name: &str, reload: Option<Json>) -> Reader {
    Reader {
        name: name.to_owned(),
        contract: format!("{name}.json"),
        image: format!("ghcr.io/x/{name}"),
        digest: "sha256:00".to_owned(),
        app: name.to_owned(),
        version: "1.0.0".to_owned(),
        documents: vec!["server".to_owned()],
        reload,
    }
}

fn with_class(class: &str) -> Json {
    let mut entry = key("database.url").occurrences[0].1.clone();
    entry.insert("reload".to_owned(), json!(class));
    Json::Object(entry)
}

fn rebuild() -> Json {
    json!({"mode": "rebuild", "layers": ["document", "secrets_dir"]})
}

#[test]
fn a_setting_says_what_a_change_to_it_costs() {
    let live = setting("database.url", &[("api", with_class("live"))]);
    let text = full(&live, &[reader("api", Some(rebuild()))], &dialect(), 1).join("\n");
    assert!(
        text.contains("on change     applied by a rebuild"),
        "{text}"
    );
    assert!(text.contains("watched file (document,"), "{text}");
    assert!(text.contains("secrets_dir);"), "{text}");

    let undeclared = full(&key("database.url"), &[reader("api", None)], &dialect(), 1).join("\n");
    assert!(
        undeclared.contains("rolls the pods — the image declares nothing about reloading"),
        "{undeclared}"
    );
}

#[test]
fn two_images_that_disagree_are_each_named() {
    let shared = setting(
        "database.url",
        &[("api", with_class("live")), ("worker", with_class("live"))],
    );
    let readers = [reader("api", Some(rebuild())), reader("worker", None)];
    let text = full(&shared, &readers, &dialect(), 2).join("\n");
    assert!(text.contains("on change     the images disagree"), "{text}");
    assert!(text.contains("api: applied by a rebuild"), "{text}");
    assert!(text.contains("worker: rolls the pods"), "{text}");
}

#[test]
fn a_class_this_build_does_not_know_is_a_restart_that_says_so() {
    let later = setting("database.url", &[("api", with_class("eventually"))]);
    let text = full(&later, &[reader("api", Some(rebuild()))], &dialect(), 1).join("\n");
    assert!(text.contains("read as a restart"), "{text}");
}

#[test]
fn every_spelling_the_loader_accepts_is_shown() {
    let text = rendered("database.url", 1);
    assert!(text.contains("FIXTURE_DATABASE__URL"), "{text}");
    assert!(text.contains("FIXTURE_DATABASE__URL_FILE"), "{text}");
    assert!(text.contains("database__url"), "{text}");
    assert!(text.contains("from a file   yes"), "{text}");
}

#[test]
fn a_key_no_file_can_supply_says_so_beside_the_spellings() {
    // The file spellings exist for every key, and for anything but a text key neither works: a file
    // delivers a string and a loader will not coerce one. Printing the spelling without printing
    // that is how an operator ends up mounting a Secret that is silently never read.
    let text = rendered("auth.session_ttl", 1);
    assert!(text.contains("FIXTURE_AUTH__SESSION_TTL_FILE"), "{text}");
    assert!(text.contains("from a file   no"), "{text}");
    assert!(text.contains("read as integer"), "{text}");
}

#[test]
fn the_markers_and_the_enum_reach_the_entry() {
    let text = rendered("database.url", 1);
    assert!(text.contains("secret        yes"), "{text}");
    assert!(text.contains("required      yes"), "{text}");
    assert!(
        rendered("log.level", 1).contains("debug | info | warn"),
        "{}",
        rendered("log.level", 1)
    );
}

#[test]
fn a_single_image_chart_does_not_attribute_readers() {
    assert!(!rendered("database.url", 1).contains("read by"));
    assert!(rendered("database.url", 2).contains("read by"));
}

// -- the compact listing -----------------------------------------------------------------

fn rows(paths: &[&str], total: usize) -> Vec<String> {
    let settings: Vec<Setting> = paths.iter().map(|path| key(path)).collect();
    let held: Vec<&Setting> = settings.iter().collect();
    compact(&held, total)
}

#[test]
fn required_and_secret_are_two_markers_in_one_column() {
    assert!(rows(&["database.url"], 1)[0].contains("RS"));
    assert!(rows(&["log.level"], 1)[0].contains(".."));
}

#[test]
fn the_default_column_disappears_when_nothing_has_one() {
    // A producer that publishes no defaults leaves 167 rows of a column holding only dashes,
    // which costs width and says nothing.
    assert!(!rows(&["log.level"], 1)[0].contains('-'));
    assert!(rows(&["auth.session_ttl"], 1)[0].contains("3600"));
}
