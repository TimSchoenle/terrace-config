use std::collections::BTreeMap;

use serde_json::{Value as Json, json};

use super::{
    Carrier, Case, DISTINCTIVE_INTEGER, Plan, Probe, Route, Target, VALUES_ROOT, document_pattern,
    escape, plan, prerequisite_conflict, probe_for, render_suite, selector_path, toml_key,
    toml_scalar, unsupplied_entries, values_path,
};
use crate::union::{Merged, Ordered, Union, union_contracts};

/// One contract key, with the fields every entry carries and no others.
fn key(path: &str, overrides: &Json) -> Merged {
    let mut fields = json!({
        "path": path,
        "env": format!("APP_{}", path.to_uppercase().replace('.', "__")),
        "text_form": "text",
        "constraint": {"type": "string"},
        "default_value": null,
        "required": false,
        "secret": false,
        "reserved": false,
        "values": [],
    });
    for (name, value) in overrides.as_object().expect("an object") {
        fields[name] = value.clone();
    }
    Merged {
        name: path.to_owned(),
        fields: fields.as_object().expect("an object").clone(),
        sources: vec!["contracts/one.json".to_owned()],
    }
}

/// The three keys most of these plan over: one probeable integer, one text, one credential.
fn keys() -> Vec<Merged> {
    vec![
        key(
            "isr.ttl_secs",
            &json!({
                "text_form": "integer",
                "constraint": {"type": "integer", "minimum": 0},
                "default_value": 0,
            }),
        ),
        key("assets.dist_dir", &json!({"default_value": "public"})),
        key("database.password", &json!({"secret": true})),
    ]
}

fn planned(
    keys: &[Merged],
    baseline: &[(String, Json)],
    prerequisites: &[(String, Json)],
    root: &str,
    routes: &BTreeMap<String, Route>,
) -> Plan {
    let held: Vec<&Merged> = keys.iter().collect();
    plan(&held, baseline, prerequisites, root, routes).expect("the plan is buildable")
}

fn simple(keys: &[Merged]) -> Plan {
    planned(keys, &[], &[], VALUES_ROOT, &BTreeMap::new())
}

fn case_for<'a>(plan: &'a Plan, path: &str) -> &'a Case {
    plan.cases
        .iter()
        .find(|case| case.path == path)
        .expect("a case for that key")
}

fn probe(key: &Merged) -> Probe {
    probe_for(key, None)
        .expect("the key is readable")
        .expect("the key carries a probe")
}

fn refusal(key: &Merged) -> String {
    probe_for(key, None)
        .expect("the key is readable")
        .expect_err("the key carries no probe")
}

// -- choosing a probe --------------------------------------------------------------------

#[test]
fn a_boolean_probe_is_the_opposite_of_the_default() {
    let found = probe(&key(
        "csp.hashes",
        &json!({"text_form": "boolean", "constraint": {"type": "boolean"},
                    "default_value": true}),
    ));
    assert_eq!(found.value, json!(false));
    assert_eq!(found.text, "false");
}

#[test]
fn a_boolean_with_no_default_still_gets_a_probe() {
    let found = probe(&key(
        "csp.hashes",
        &json!({"text_form": "boolean", "constraint": {"type": "boolean"}}),
    ));
    assert_eq!(found.value, json!(true));
}

#[test]
fn an_integer_probe_is_distinctive_rather_than_adjacent_to_the_default() {
    // A passing assertion should not be explicable as a coincidence, and `1` is a value half
    // the charts in a repository render somewhere by accident.
    let found = probe(&key(
        "isr.ttl",
        &json!({"text_form": "integer", "constraint": {"type": "integer", "minimum": 0},
                    "default_value": 0}),
    ));
    assert_eq!(found.value, json!(DISTINCTIVE_INTEGER));
}

#[test]
fn an_integer_probe_respects_the_bounds_the_contract_publishes() {
    let found = probe(&key(
        "server.port",
        &json!({"text_form": "integer",
                    "constraint": {"type": "integer", "minimum": 1, "maximum": 99},
                    "default_value": 8080}),
    ));
    let held = found.value.as_i64().expect("an integer probe");
    assert!((1..=99).contains(&held), "{held}");

    let found = probe(&key(
        "tuning.step",
        &json!({"text_form": "integer",
                    "constraint": {"type": "integer", "minimum": 0, "multipleOf": 5},
                    "default_value": 0}),
    ));
    let held = found.value.as_i64().expect("an integer probe");
    assert_eq!(held % 5, 0, "{held}");
    assert_ne!(held, 0);
}

#[test]
fn a_text_probe_names_the_key_it_probes() {
    // The rendered document shows which setting went missing without a reader opening the
    // contract.
    let found = probe(&key("assets.dist_dir", &json!({"default_value": "public"})));
    assert_eq!(found.value, json!("contract-probe-assets-dist-dir"));
    assert_eq!(found.text, "\"contract-probe-assets-dist-dir\"");
}

#[test]
fn a_text_probe_is_shortened_to_fit_a_maximum_length() {
    let found = probe(&key(
        "assets.dist_dir",
        &json!({"constraint": {"type": "string", "maxLength": 12}}),
    ));
    assert_eq!(found.value.as_str().expect("a text probe").len(), 12);
}

#[test]
fn the_environment_spelling_steers_the_choice() {
    // A probe the file could carry and a variable could not is not deliverable both ways.
    let found = probe(&key(
        "isr.ttl",
        &json!({"text_form": "integer", "constraint": {"type": "integer", "minimum": 0},
                    "text_constraint": {"type": "string", "pattern": r"^\s*[0-9]{2}\s*$"}}),
    ));
    assert_ne!(found.value, json!(DISTINCTIVE_INTEGER));
    let held = found.value.as_i64().expect("an integer probe");
    assert!((10..=99).contains(&held), "{held}");
}

#[test]
fn a_choice_probe_is_a_declared_value_other_than_the_default() {
    let found = probe(&key(
        "rate_limit.backend",
        &json!({"text_form": "choice", "values": ["memory", "redis"],
                    "constraint": {"type": "string", "enum": ["memory", "redis"]},
                    "default_value": "memory"}),
    ));
    assert_eq!(found.value, json!("redis"));
}

// -- probes that must not be invented ----------------------------------------------------

#[test]
fn a_credential_is_refused_because_a_committed_probe_is_a_credential() {
    assert!(
        refusal(&key("database.password", &json!({"secret": true}))).contains("secret"),
        "{}",
        refusal(&key("database.password", &json!({"secret": true})))
    );
}

#[test]
fn the_two_unprobeable_forms_are_refused_for_their_own_reasons() {
    // Three different situations, and a reader is never left to guess which applies.
    let structured = refusal(&key(
        "internal.peers",
        &json!({"text_form": "structured", "constraint": {"type": "object"}}),
    ));
    assert!(structured.contains("structured"), "{structured}");
    assert!(structured.contains("operator's own names"), "{structured}");

    let unknown = refusal(&key(
        "tuning.ratio",
        &json!({"text_form": "unknown", "constraint": {}}),
    ));
    assert!(unknown.contains("no constraint"), "{unknown}");
}

#[test]
fn a_key_no_value_but_its_default_satisfies_carries_no_probe() {
    // A case that sets a key to the value it already has reports a proof it has not earned.
    for (path, overrides) in [
        (
            "app.mode",
            json!({"constraint": {"type": "string", "const": "server"},
                       "default_value": "server"}),
        ),
        (
            "app.mode",
            json!({"text_form": "choice", "values": ["server"],
                       "constraint": {"type": "string", "enum": ["server"]},
                       "default_value": "server"}),
        ),
    ] {
        let reason = refusal(&key(path, &overrides));
        assert!(reason.contains("differing from the default"), "{reason}");
    }
}

#[test]
fn a_text_key_constrained_by_a_pattern_carries_no_probe() {
    let reason = refusal(&key(
        "app.slug",
        &json!({"constraint": {"type": "string", "pattern": "^[0-9]{4}$"}}),
    ));
    assert!(reason.contains("pattern"), "{reason}");
}

#[test]
fn a_constraint_outside_the_vocabulary_carries_no_probe() {
    let reason = refusal(&key(
        "app.slug",
        &json!({"constraint": {"type": "string", "allOf": [{"minLength": 1}]}}),
    ));
    assert!(reason.contains("allOf"), "{reason}");
}

#[test]
fn a_probe_no_environment_spelling_can_carry_is_refused() {
    // `text_constraint` governs the variable, and a key has to be deliverable both ways.
    let reason = refusal(&key(
        "isr.ttl",
        &json!({"text_form": "integer", "constraint": {"type": "integer"},
                    "text_constraint": {"type": "string", "pattern": r"^\s*[0-9]{40}\s*$"}}),
    ));
    assert!(reason.contains("differing from the default"), "{reason}");
}

// -- spelling the assertion --------------------------------------------------------------

const DOCUMENT: &str = "[assets]\ndist_dir = \"public\"\n\n[csp]\nhash_inline_scripts = true\n\
                            \n[csp.cloudflare]\nscript_nonce = true\nturnstile = false\n\
                            web_analytics = false\n\n[isr]\ncache_dir = \"/tmp/isr\"\n\
                            ttl_secs = 0\n";

fn matches(path: &str, text: &str) -> bool {
    let pattern = document_pattern(path, text).expect("a spellable path");
    regex::Regex::new(&pattern)
        .expect("the generated pattern compiles")
        .is_match(DOCUMENT)
}

#[test]
fn a_leaf_under_its_own_table_matches() {
    assert!(matches("csp.cloudflare.turnstile", "false"));
    assert!(matches("csp.hash_inline_scripts", "true"));
    assert!(matches("isr.ttl_secs", "0"));
    assert!(matches("csp.cloudflare.script_nonce", "true"));
}

#[test]
fn a_leaf_under_the_wrong_table_does_not_match() {
    // The defect a pattern keyed on the leaf name alone would wave through.
    assert!(!matches("isr.turnstile", "false"));
    assert!(!matches("csp.turnstile", "false"));
}

#[test]
fn a_wrong_value_or_a_suffix_of_another_leaf_does_not_match() {
    assert!(!matches("csp.cloudflare.turnstile", "true"));
    assert!(!matches("isr.secs", "0"));
}

#[test]
fn a_quoted_value_is_matched_as_the_renderer_writes_it() {
    assert!(matches("isr.cache_dir", &toml_scalar(&json!("/tmp/isr"))));
}

#[test]
fn only_shared_metacharacters_are_escaped() {
    // A general escaper also escapes punctuation Go's parser is not obliged to accept.
    assert_eq!(escape("a-b~c#d e"), "a-b~c#d e");
    assert_eq!(escape("[a].b"), r"\[a\]\.b");
}

#[test]
fn a_key_outside_toml_s_bare_alphabet_is_quoted() {
    assert_eq!(toml_key("docs/handbook"), "\"docs/handbook\"");
    assert_eq!(toml_key("dist_dir"), "dist_dir");
}

#[test]
fn a_top_level_key_is_anchored_at_the_document() {
    let pattern = document_pattern("level", "\"info\"").expect("a spellable path");
    let compiled = regex::Regex::new(&pattern).expect("it compiles");
    assert!(compiled.is_match("level = \"info\"\n\n[isr]\nttl_secs = 0\n"));
    assert!(!compiled.is_match("[isr]\nlevel = \"info\"\n"));
}

#[test]
fn a_path_with_an_empty_segment_is_refused_rather_than_spelled() {
    assert!(document_pattern("a..b", "1").is_err());
}

// -- spelling the document selector ------------------------------------------------------

fn label(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

#[test]
fn a_key_that_identifies_its_document_selects_on_the_key_alone() {
    assert_eq!(
        selector_path("config.toml", &[]).expect("a spellable key"),
        "data[\"config.toml\"]"
    );
}

#[test]
fn a_shared_key_is_narrowed_by_the_labels_the_declaration_selects_on() {
    assert_eq!(
        selector_path(
            "config.toml",
            &[label("app.kubernetes.io/component", "api")]
        )
        .expect("a spellable key"),
        "$[?(@.metadata.labels[\"app.kubernetes.io/component\"]==\"api\")].data\
             [\"config.toml\"]"
    );
}

#[test]
fn several_labels_become_one_filter() {
    // The framework's document selector carries one path and one value, and no more.
    assert_eq!(
        selector_path("config.toml", &[label("a", "1"), label("b", "2")]).expect("a spellable key"),
        "$[?(@.metadata.labels[\"a\"]==\"1\" && @.metadata.labels[\"b\"]==\"2\")].data\
             [\"config.toml\"]"
    );
}

#[test]
fn a_label_no_string_literal_can_carry_is_refused_rather_than_escaped() {
    // A guess at another engine's lexer produces a selector that silently matches nothing, and a
    // line break would be folded into a space by the single-quoted YAML scalar it is written into.
    for offender in [
        "quo\"te",
        "back\\slash",
        "line\nbreak",
        "carriage\rreturn",
        "t\tab",
    ] {
        assert!(
            selector_path(
                "config.toml",
                &[label("app.kubernetes.io/component", offender)]
            )
            .is_err(),
            "{offender:?}"
        );
        assert!(selector_path(offender, &[]).is_err(), "{offender:?}");
    }
}

// -- planning ----------------------------------------------------------------------------

#[test]
fn cases_and_skips_are_sorted_by_contract_path() {
    let plan = simple(&keys());
    assert_eq!(
        plan.cases.iter().map(|case| &case.path).collect::<Vec<_>>(),
        ["assets.dist_dir", "isr.ttl_secs"]
    );
    assert_eq!(
        plan.skipped
            .iter()
            .map(|held| &held.path)
            .collect::<Vec<_>>(),
        ["database.password"]
    );
}

#[test]
fn a_case_writes_its_key_under_the_raw_configuration_tree() {
    let plan = simple(&keys());
    let case = case_for(&plan, "isr.ttl_secs");
    assert_eq!(
        case.set_values.last(),
        Some(&("config.isr.ttl_secs".to_owned(), json!(DISTINCTIVE_INTEGER)))
    );
}

#[test]
fn the_baseline_is_carried_by_every_case() {
    let baseline = vec![(
        "config.csp.cloudflare.script_nonce".to_owned(),
        json!(false),
    )];
    for case in planned(&keys(), &baseline, &[], VALUES_ROOT, &BTreeMap::new()).cases {
        assert!(case.set_values.contains(&baseline[0]), "{case:?}");
    }
}

#[test]
fn the_baseline_never_supplies_the_value_the_case_is_probing() {
    // Otherwise the case passes whether or not the chart delivered anything.
    let probed = values_path("isr.ttl_secs", VALUES_ROOT);
    let baseline = vec![(probed.clone(), json!(99))];
    let plan = planned(&keys(), &baseline, &[], VALUES_ROOT, &BTreeMap::new());
    let case = case_for(&plan, "isr.ttl_secs");
    assert_eq!(
        case.set_values
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [probed.as_str()]
    );
    assert_eq!(case.set_values[0].1, json!(DISTINCTIVE_INTEGER));
}

// -- routed probes -----------------------------------------------------------------------

fn route(values_path: &str, condition: Option<&str>, schema: Option<Json>) -> Route {
    Route {
        values_path: values_path.to_owned(),
        condition: condition.map(str::to_owned),
        schema,
    }
}

fn routes() -> BTreeMap<String, Route> {
    BTreeMap::from([
        (
            "assets.dist_dir".to_owned(),
            route("assets.distDir", None, None),
        ),
        ("isr.ttl_secs".to_owned(), route("isr.ttlSecs", None, None)),
    ])
}

#[test]
fn the_probe_lands_on_the_chart_value_when_a_marker_names_one() {
    // The difference between proving the escape hatch is merged and proving the chart's own
    // mapping: a typo in the helper between the two spellings passes every unrouted case.
    let plan = planned(&keys(), &[], &[], VALUES_ROOT, &routes());
    let case = case_for(&plan, "isr.ttl_secs");
    assert_eq!(
        case.set_values.last(),
        Some(&("isr.ttlSecs".to_owned(), json!(DISTINCTIVE_INTEGER)))
    );
    assert_eq!(case.through.as_deref(), Some("isr.ttlSecs"));
}

#[test]
fn a_key_with_no_route_still_goes_through_the_raw_tree() {
    let plan = simple(&keys());
    let case = case_for(&plan, "isr.ttl_secs");
    assert_eq!(
        case.set_values.last(),
        Some(&("config.isr.ttl_secs".to_owned(), json!(DISTINCTIVE_INTEGER)))
    );
    assert!(case.through.is_none());
}

#[test]
fn a_gate_the_marker_names_is_switched_on() {
    // A `when` clause is a values path the chart tests before writing the key at all, so a
    // case that left it alone would assert against a document the probe never reached.
    let routes = BTreeMap::from([(
        "isr.ttl_secs".to_owned(),
        route("isr.ttlSecs", Some("isr.enabled"), None),
    )]);
    let plan = planned(&keys(), &[], &[], VALUES_ROOT, &routes);
    assert!(
        case_for(&plan, "isr.ttl_secs")
            .set_values
            .contains(&("isr.enabled".to_owned(), json!(true)))
    );
}

#[test]
fn the_baseline_is_still_dropped_by_its_escape_hatch_path() {
    // The raw tree is merged over what the chart derives, so a baseline entry naming the key a
    // case probes would override the chart value the probe was just written into.
    let baseline = vec![(values_path("isr.ttl_secs", VALUES_ROOT), json!(99))];
    let plan = planned(&keys(), &baseline, &[], VALUES_ROOT, &routes());
    assert_eq!(
        case_for(&plan, "isr.ttl_secs")
            .set_values
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["isr.ttlSecs"]
    );
}

#[test]
fn a_chart_schema_nothing_satisfies_falls_back_to_the_raw_tree() {
    // A chart value may be typed more tightly than the contract, deliberately. A probe that
    // satisfies the contract and not the chart would fail the templating engine's own values
    // validation, so the case goes through the untyped escape hatch rather than disappearing.
    let routes = BTreeMap::from([(
        "isr.ttl_secs".to_owned(),
        route("isr.ttlSecs", None, Some(json!({"maximum": -1}))),
    )]);
    let plan = planned(&keys(), &[], &[], VALUES_ROOT, &routes);
    let case = case_for(&plan, "isr.ttl_secs");
    assert_eq!(
        case.set_values.last(),
        Some(&("config.isr.ttl_secs".to_owned(), json!(DISTINCTIVE_INTEGER)))
    );
    assert!(case.through.is_none());

    let routes = BTreeMap::from([(
        "isr.ttl_secs".to_owned(),
        route(
            "isr.ttlSecs",
            None,
            Some(json!({"type": "integer", "minimum": 1})),
        ),
    )]);
    let plan = planned(&keys(), &[], &[], VALUES_ROOT, &routes);
    assert_eq!(
        case_for(&plan, "isr.ttl_secs").through.as_deref(),
        Some("isr.ttlSecs")
    );
}

// -- where a probe is written ------------------------------------------------------------

const MOVED: &str = "services.api.config";

#[test]
fn a_case_writes_its_key_under_the_root_the_enrolment_names() {
    let plan = planned(&keys(), &[], &[], MOVED, &BTreeMap::new());
    assert_eq!(
        case_for(&plan, "isr.ttl_secs").set_values.last(),
        Some(&(format!("{MOVED}.isr.ttl_secs"), json!(DISTINCTIVE_INTEGER)))
    );
    assert_eq!(
        values_path("isr.ttl_secs", VALUES_ROOT),
        "config.isr.ttl_secs"
    );
}

#[test]
fn the_baseline_collision_is_still_caught_under_a_moved_root() {
    // A baseline one layer off the probes would silently supply the value under test.
    let probed = values_path("isr.ttl_secs", MOVED);
    let plan = planned(
        &keys(),
        &[(probed.clone(), json!(99))],
        &[],
        MOVED,
        &BTreeMap::new(),
    );
    assert_eq!(
        case_for(&plan, "isr.ttl_secs")
            .set_values
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [probed.as_str()]
    );
}

// -- render prerequisites ----------------------------------------------------------------

fn prerequisites() -> Vec<(String, Json)> {
    vec![(
        "bucket.entries".to_owned(),
        json!({"link": {"bucket": "b", "object": "o"}}),
    )]
}

#[test]
fn every_case_carries_the_prerequisites_and_no_case_drops_them() {
    // Unlike a baseline: a case missing what the chart's guard insists on does not render, and
    // a case that does not render proves nothing either.
    let held = prerequisites();
    for case in planned(&keys(), &[], &held, VALUES_ROOT, &BTreeMap::new()).cases {
        assert_eq!(case.set_values.first(), Some(&held[0]), "{}", case.path);
    }

    let collides = vec![(values_path("isr.ttl_secs", VALUES_ROOT), json!(99))];
    let held = vec![("server.replicas".to_owned(), json!(2))];
    for case in planned(&keys(), &collides, &held, VALUES_ROOT, &BTreeMap::new()).cases {
        assert!(case.set_values.contains(&held[0]), "{}", case.path);
    }
}

fn refined_map(required: bool) -> Merged {
    key(
        "legal.documents",
        &json!({
            "text_form": "structured",
            "required": required,
            "constraint": {
                "type": "object",
                "additionalProperties": {"type": "object"},
                "required": ["imprint", "privacy"],
            },
        }),
    )
}

/// [`unsupplied_entries`] with no prerequisites and no carriers: the tree is the only source.
fn unsupplied_in_tree(
    keys: &[&Merged],
    baseline: &[(String, Json)],
    values: &Json,
) -> Result<(), crate::error::Error> {
    unsupplied_entries(keys, baseline, &[], values, VALUES_ROOT, &BTreeMap::new())
}

/// `legal.documents` carried by the chart value `legal.documents`, as a `structured` marker
/// binding the two would state it.
fn legal_carrier(condition: Option<&str>) -> BTreeMap<String, Carrier> {
    BTreeMap::from([(
        "legal.documents".to_owned(),
        Carrier {
            values_path: "legal.documents".to_owned(),
            condition: condition.map(str::to_owned),
        },
    )])
}

fn both_documents() -> Vec<(String, Json)> {
    vec![
        (
            "legal.documents.imprint.title.en".to_owned(),
            json!("Imprint"),
        ),
        (
            "legal.documents.privacy.title.en".to_owned(),
            json!("Privacy"),
        ),
    ]
}

#[test]
fn a_required_map_nothing_supplies_fails_generation_naming_every_entry() {
    let map = refined_map(true);
    let failure = unsupplied_in_tree(&[&map], &[], &json!({"config": {}}))
        .expect_err("nothing supplies the entries");
    let message = failure.to_string();
    assert!(
        message.contains(
            "operator must supply fixture for legal.documents: must contain imprint, privacy"
        ),
        "{message}"
    );
    assert!(
        message.contains("unsupplied: imprint, privacy"),
        "{message}"
    );

    // The structured key still carries no probe: the refusal is about the fixture, not a case.
    assert_eq!(simple(std::slice::from_ref(&map)).cases, Vec::new());
}

#[test]
fn a_required_map_is_supplied_by_a_baseline_leaf_or_the_charts_own_values() {
    let map = refined_map(true);
    let baseline = vec![(
        format!("{VALUES_ROOT}.legal.documents.imprint.title"),
        json!("Imprint"),
    )];
    let values = json!({"config": {"legal": {"documents": {"privacy": {"title": "Privacy"}}}}});
    unsupplied_in_tree(&[&map], &baseline, &values)
        .expect("one entry from each source is every entry");

    let half = unsupplied_in_tree(&[&map], &baseline, &json!({}))
        .expect_err("privacy is still unsupplied")
        .to_string();
    assert!(half.contains("unsupplied: privacy"), "{half}");
}

#[test]
fn a_required_map_is_supplied_by_the_prerequisites_through_the_chart_value_carrying_it() {
    // The chart's own schema demands both documents, so the enrolment states them once, as the
    // chart value, and the template copies that value into the key.
    let map = refined_map(true);
    unsupplied_entries(
        &[&map],
        &[],
        &both_documents(),
        &json!({"legal": {"documents": {}}}),
        VALUES_ROOT,
        &legal_carrier(None),
    )
    .expect("the carrier delivers both entries");

    // A prerequisite naming the map itself, rather than one leaf per line, carries them too.
    let whole = vec![(
        "legal.documents".to_owned(),
        json!({"imprint": {"title": {"en": "I"}}, "privacy": {"title": {"en": "P"}}}),
    )];
    unsupplied_entries(
        &[&map],
        &[],
        &whole,
        &json!({}),
        VALUES_ROOT,
        &legal_carrier(None),
    )
    .expect("a map-valued prerequisite holds its entries");

    // So does the chart's own default for the value.
    let defaulted = json!({"legal": {"documents": {"imprint": {}, "privacy": {}}}});
    unsupplied_entries(
        &[&map],
        &[],
        &[],
        &defaulted,
        VALUES_ROOT,
        &legal_carrier(None),
    )
    .expect("a chart default carried into the key supplies it");
}

#[test]
fn a_prerequisite_reaches_the_key_only_through_a_carrier() {
    // Without a marker binding the value to the key, nothing says the template copies it.
    let failure = unsupplied_entries(
        &[&refined_map(true)],
        &[],
        &both_documents(),
        &json!({}),
        VALUES_ROOT,
        &BTreeMap::new(),
    )
    .expect_err("an unbound chart value supplies nothing")
    .to_string();
    assert!(
        failure.contains("unsupplied: imprint, privacy"),
        "{failure}"
    );
    assert!(!failure.contains("prerequisites.values"), "{failure}");
}

#[test]
fn a_carrier_behind_a_when_clause_counts_only_while_the_render_switches_it_on() {
    let map = refined_map(true);
    let gated = legal_carrier(Some("legal.enabled"));

    let off = unsupplied_entries(
        &[&map],
        &[],
        &both_documents(),
        &json!({"legal": {"enabled": false}}),
        VALUES_ROOT,
        &gated,
    )
    .expect_err("the chart never writes the key while the gate is off")
    .to_string();
    assert!(off.contains("unsupplied: imprint, privacy"), "{off}");
    assert!(
        off.contains("or under `legal.documents` in its `prerequisites.values`"),
        "{off}"
    );

    let mut on = both_documents();
    on.push(("legal.enabled".to_owned(), json!(true)));
    unsupplied_entries(&[&map], &[], &on, &json!({}), VALUES_ROOT, &gated)
        .expect("a prerequisite switching the gate on lets the carrier count");

    unsupplied_entries(
        &[&map],
        &[],
        &both_documents(),
        &json!({"legal": {"enabled": true}}),
        VALUES_ROOT,
        &gated,
    )
    .expect("so does the chart's own default");
}

#[test]
fn a_null_prerequisite_supplies_nothing() {
    // `--set x=null` deletes a value in Helm; it cannot be what holds an entry.
    let nulled = vec![("legal.documents.imprint".to_owned(), Json::Null)];
    let failure = unsupplied_entries(
        &[&refined_map(true)],
        &[],
        &nulled,
        &json!({}),
        VALUES_ROOT,
        &legal_carrier(None),
    )
    .expect_err("a null entry is no entry")
    .to_string();
    assert!(
        failure.contains("unsupplied: imprint, privacy"),
        "{failure}"
    );
}

#[test]
fn a_refined_map_whose_default_holds_its_entries_asks_nothing() {
    // Published `required: false`: the image's own default supplies what the chart does not.
    unsupplied_in_tree(&[&refined_map(false)], &[], &json!({}))
        .expect("a default the image supplies needs no fixture");
}

#[test]
fn a_prerequisite_inside_the_probed_tree_is_refused_by_the_model() {
    // The guarantee, asserted where a caller reaching past the loader cannot reach past it.
    let all = keys();
    let held: Vec<&Merged> = all.iter().collect();
    for path in [
        VALUES_ROOT.to_owned(),
        format!("{VALUES_ROOT}.isr.ttl_secs"),
    ] {
        let failure = plan(
            &held,
            &[],
            &[(path.clone(), json!(1))],
            VALUES_ROOT,
            &BTreeMap::new(),
        )
        .expect_err("a prerequisite inside the probes is refused");
        assert!(failure.to_string().contains(VALUES_ROOT), "{failure}");
    }
}

#[test]
fn the_refusal_follows_the_probe_root_rather_than_the_default_tree() {
    // The one place the two enrolment fields actually meet. On a chart probing a per-service
    // layer, `config` is the *lowest*-precedence one and a prerequisite there cannot reach a
    // probed key. A rule spelled against the default would refuse the harmless path and admit
    // the dangerous one, which is exactly backwards.
    assert!(prerequisite_conflict(VALUES_ROOT, MOVED).is_none());
    assert!(prerequisite_conflict(&format!("{VALUES_ROOT}.isr.ttl_secs"), MOVED).is_none());
    assert!(prerequisite_conflict(MOVED, MOVED).is_some());
    assert!(prerequisite_conflict(&format!("{MOVED}.isr.ttl_secs"), MOVED).is_some());

    // The refusal is on the tree, not on the six letters: `configMount` is another value.
    assert!(prerequisite_conflict("configMount.rolloutOnChange", VALUES_ROOT).is_none());
    assert!(prerequisite_conflict("", VALUES_ROOT).is_some());
}

#[test]
fn a_prerequisite_the_probe_root_nests_inside_is_refused() {
    // Two entries of one `set` mapping, one enclosing the other, and no order between them.
    let conflict = prerequisite_conflict("services.api", MOVED).expect("a conflict");
    assert!(conflict.contains("no order"), "{conflict}");
    assert!(prerequisite_conflict("services.worker.replicaCount", MOVED).is_none());
}

// -- rendering ---------------------------------------------------------------------------

fn target() -> Target {
    Target {
        chart: "portfolio".to_owned(),
        name: "server".to_owned(),
        kind: "ConfigMap".to_owned(),
        selector: vec![label("app.kubernetes.io/instance", "portfolio")],
        key: "config.toml".to_owned(),
        declaration: "charts/portfolio/config-contract.yaml".to_owned(),
        enrolment: "the chart's `contract-tests.yaml`".to_owned(),
        contracts: vec!["charts/portfolio/contracts/server.json".to_owned()],
        discriminator: Vec::new(),
        root: VALUES_ROOT.to_owned(),
    }
}

/// The other shape a chart can have: documents sharing one key, probed through the layer that
/// outranks the configuration tree rather than through the tree itself.
fn shared_target() -> Target {
    Target {
        chart: "tankovault".to_owned(),
        name: "api".to_owned(),
        kind: "ConfigMap".to_owned(),
        selector: vec![label("app.kubernetes.io/component", "api")],
        key: "config.toml".to_owned(),
        declaration: "charts/tankovault/config-contract.yaml".to_owned(),
        enrolment: "the chart's `contract-tests.yaml`".to_owned(),
        contracts: vec!["charts/tankovault/contracts/api.json".to_owned()],
        discriminator: vec![label("app.kubernetes.io/component", "api")],
        root: MOVED.to_owned(),
    }
}

fn render(
    target: &Target,
    plan: &Plan,
    baseline: &[(String, Json)],
    reason: Option<&str>,
) -> String {
    render_suite(target, plan, baseline, reason, &[], None, &[]).expect("the suite is renderable")
}

fn parsed(text: &str) -> Json {
    let held: serde_norway::Value = serde_norway::from_str(text).expect("the suite is valid YAML");
    serde_json::to_value(held).expect("YAML converts to JSON")
}

#[test]
fn the_suite_is_valid_yaml_shaped_like_a_test_suite() {
    let suite = parsed(&render(&target(), &simple(&keys()), &[], None));
    assert_eq!(suite["release"]["name"], json!("portfolio"));
    let tests = suite["tests"].as_array().expect("a list of cases");
    assert_eq!(tests.len(), 3);
    assert!(
        tests[0]["it"]
            .as_str()
            .expect("a title")
            .starts_with("renders"),
        "{}",
        tests[0]["it"]
    );
}

#[test]
fn the_pattern_survives_yaml_with_its_backslashes_intact() {
    // A double-quoted scalar would consume them as YAML escapes before the regex engine saw
    // them, and every pattern here is dense with backslashes.
    let suite = parsed(&render(&target(), &simple(&keys()), &[], None));
    let held = suite["tests"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|case| {
            case["it"]
                .as_str()
                .is_some_and(|it| it.contains("isr.ttl_secs"))
        })
        .expect("a case for the integer key");
    assert_eq!(
        held["asserts"][0]["matchRegex"]["pattern"],
        json!(
            document_pattern("isr.ttl_secs", &DISTINCTIVE_INTEGER.to_string())
                .expect("a spellable path")
        )
    );
}

#[test]
fn the_header_says_the_file_is_generated_and_names_its_sources() {
    let rendered = render(&target(), &simple(&keys()), &[], None);
    assert!(rendered.starts_with("# Generated by"), "{rendered}");
    assert!(rendered.contains("charts/portfolio/contracts/server.json"));
    assert!(rendered.contains("charts/portfolio/config-contract.yaml"));
}

#[test]
fn every_skipped_key_is_named_in_the_file_with_its_reason() {
    // An unexplained absence is indistinguishable from an oversight, which for a file whose
    // whole job is to be exhaustive is the worst possible failure mode.
    let rendered = render(&target(), &simple(&keys()), &[], None);
    assert!(rendered.contains("database.password:"), "{rendered}");
    assert!(rendered.contains("credential"), "{rendered}");
}

#[test]
fn nothing_that_churns_on_its_own_is_written_into_the_file() {
    // A digest, a timestamp or an application version would put a diff here on every bump, and
    // the staleness gate is a comparison of bytes.
    let baseline = vec![("config.a.b".to_owned(), json!(false))];
    let plan = planned(&keys(), &baseline, &[], VALUES_ROOT, &BTreeMap::new());
    let first = render(&target(), &plan, &baseline, Some("because"));
    assert_eq!(first, render(&target(), &plan, &baseline, Some("because")));
    assert!(!first.contains("sha256"), "{first}");
    assert!(
        !regex::Regex::new(r"\d{4}-\d{2}-\d{2}T")
            .expect("it compiles")
            .is_match(&first),
        "{first}"
    );
}

#[test]
fn every_case_of_a_shared_key_selects_through_the_same_filter() {
    // A case reading a sibling's document would assert a key against the wrong file.
    let target = shared_target();
    let plan = planned(&keys(), &[], &[], &target.root, &BTreeMap::new());
    let suite = parsed(&render(&target, &plan, &[], Some("because")));
    let expected = selector_path(&target.key, &target.discriminator).expect("a spellable key");
    for case in suite["tests"].as_array().expect("a list") {
        assert_eq!(
            case["documentSelector"],
            json!({"path": expected}),
            "{case}"
        );
    }
}

#[test]
fn a_shared_key_still_asserts_its_document_against_the_declaration() {
    let target = shared_target();
    let plan = planned(&keys(), &[], &[], &target.root, &BTreeMap::new());
    let suite = parsed(&render(&target, &plan, &[], Some("because")));
    assert_eq!(
        suite["tests"][0]["asserts"],
        json!([
            {"isKind": {"of": "ConfigMap"}},
            {"equal": {
                "path": "metadata.labels[\"app.kubernetes.io/component\"]",
                "value": "api",
            }},
        ])
    );
}

#[test]
fn a_moved_probe_root_reaches_the_header_the_set_paths_and_the_note() {
    let target = shared_target();
    let plan = planned(&keys(), &[], &[], &target.root, &BTreeMap::new());
    let rendered = render(&target, &plan, &[], Some("the derived wiring outranks it"));
    assert!(
        rendered.contains("`services.api.config` where no value is bound"),
        "{rendered}"
    );
    assert!(
        rendered.contains("services.api.config.isr.ttl_secs:"),
        "{rendered}"
    );
    assert!(rendered.contains("rather than into `config`"), "{rendered}");
    assert!(
        rendered.contains("the derived wiring outranks it"),
        "{rendered}"
    );
}

#[test]
fn a_document_with_neither_a_baseline_nor_a_moved_root_carries_no_note() {
    // The reason belongs to what the enrolment changed; with nothing changed it is noise.
    let rendered = render(&target(), &simple(&keys()), &[], None);
    assert!(
        !rendered.contains("Every case below also carries"),
        "{rendered}"
    );
    assert!(!rendered.contains("rather than into"), "{rendered}");
}

#[test]
fn the_case_title_names_the_value_the_probe_went_through() {
    // The two kinds of case prove different things and a reader scanning the suite has no
    // other way to tell them apart.
    let plan = planned(&keys(), &[], &[], VALUES_ROOT, &routes());
    let rendered = render(&target(), &plan, &[], None);
    assert!(
        rendered.contains("delivers isr.ttl_secs into config.toml from isr.ttlSecs"),
        "{rendered}"
    );
    assert!(
        rendered.contains("2 of 3 contract keys carry a probe, 2 through the chart's own value"),
        "{rendered}"
    );
}

#[test]
fn an_unrouted_key_is_explained_in_the_suite() {
    let unrouted = vec![(
        "isr.ttl_secs".to_owned(),
        "the chart refuses a value this probe could take".to_owned(),
    )];
    let rendered = render_suite(&target(), &simple(&keys()), &[], None, &[], None, &unrouted)
        .expect("the suite is renderable");
    assert!(rendered.contains("isr.ttl_secs:"), "{rendered}");
    assert!(
        rendered.contains("the chart refuses a value this probe could take"),
        "{rendered}"
    );
}

#[test]
fn a_structured_prerequisite_survives_the_suite_as_the_value_it_was() {
    // A map of maps keyed by a request path, and a flattened one would set the wrong thing.
    let held = prerequisites();
    let plan = planned(&keys(), &[], &held, VALUES_ROOT, &BTreeMap::new());
    let rendered = render_suite(
        &target(),
        &plan,
        &[],
        None,
        &held,
        Some("because the guard insists"),
        &[],
    )
    .expect("the suite is renderable");
    let suite = parsed(&rendered);
    for case in suite["tests"].as_array().expect("a list") {
        assert_eq!(case["set"]["bucket.entries"], held[0].1, "{case}");
    }
    assert!(rendered.contains("because the guard insists"), "{rendered}");
    assert!(rendered.contains("render prerequisites"), "{rendered}");
}

#[test]
fn the_identity_case_carries_the_prerequisites_too() {
    // It asserts on a document the guard would otherwise have refused to produce.
    let held = prerequisites();
    let plan = planned(&[], &[], &held, VALUES_ROOT, &BTreeMap::new());
    let suite = parsed(
        &render_suite(
            &target(),
            &plan,
            &[],
            None,
            &held,
            Some("because the guard insists"),
            &[],
        )
        .expect("the suite is renderable"),
    );
    let tests = suite["tests"].as_array().expect("a list");
    assert_eq!(tests.len(), 1);
    assert!(!tests[0]["set"]["bucket.entries"].is_null(), "{}", tests[0]);
}

#[test]
fn a_key_a_real_contract_declares_is_probed_the_same_way_a_hand_built_one_is() {
    // The hand-built keys above carry exactly the fields a test needs. This runs the same walk
    // over a merged contract, so a field the merge adds or renames cannot go unnoticed.
    let contract: Json = serde_json::from_str(include_str!("../../../tests/fixtures/worker.json"))
        .expect("a fixture contract");
    let union: Union =
        union_contracts(&[("worker".to_owned(), contract)]).expect("it merges with itself");
    let held: &Ordered = &union.keys;
    let probed = held
        .iter()
        .filter(|key| probe_for(key, None).is_ok_and(|held| held.is_ok()))
        .count();
    assert!(probed > 0, "no key of a real contract carries a probe");
}

#[test]
fn a_set_scalar_keeps_its_yaml_type() {
    use super::render::yaml_scalar;

    assert_eq!(yaml_scalar(&json!(3)), "3");
    assert_eq!(yaml_scalar(&json!(0.5)), "0.5");
    assert_eq!(yaml_scalar(&json!(-1.25)), "-1.25");
    assert_eq!(yaml_scalar(&Json::Null), "null");
    assert_eq!(yaml_scalar(&json!(true)), "true");
    assert_eq!(yaml_scalar(&json!("0.5")), "'0.5'");
    assert_eq!(yaml_scalar(&json!("plain")), "plain");
    for word in ["true", "null", "No", "off", "Y"] {
        assert_eq!(yaml_scalar(&json!(word)), format!("'{word}'"), "{word}");
    }
}

#[test]
fn an_assertion_path_is_quoted_only_when_yaml_would_read_it_differently() {
    use super::render::yaml_path;

    assert_eq!(
        yaml_path("data[\"config.toml\"]".to_owned()),
        "data[\"config.toml\"]"
    );
    assert_eq!(
        yaml_path("metadata.labels[\"team: core\"]".to_owned()),
        "'metadata.labels[\"team: core\"]'"
    );
}
