use serde_json::{Value as Json, json};

use super::{
    Area, Kind, Severity, Status, diff_contract, observed_bump, parse_version, suggest_version,
    worst,
};

/// One vendored document: the provenance envelope a chart repository writes, and the contract.
fn vendored(contract: &Json) -> Json {
    json!({
        "source": {
            "image": "ghcr.io/x/y",
            "digest": "sha256:00",
            "sha256": "ab",
            "fetched": "2026-01-01T00:00:00Z",
        },
        "contract": contract.clone(),
    })
}

/// One published contract, with the halves a test wants to move.
fn document(keys: &Json, external: &Json) -> Json {
    json!({
        "terrace_contract": 1,
        "producer": {"name": "x", "version": "1", "loader": "figment"},
        "app": {"name": "x", "version": "1.0.0"},
        "schema": {
            "schema_version": 2,
            "dialect": {
                "prefix": "P_",
                "nesting_separator": "__",
                "indirection_suffix": "_FILE",
            },
            "loader": [],
            "keys": keys.clone(),
        },
        "json_schema": {},
        "external": {"env": external.clone(), "ignore": [], "unknown": "reject"},
    })
}

/// One key declaration, with whatever a test wants to say about it.
fn key(path: &str, extra: &Json) -> Json {
    let mut held = json!({
        "path": path,
        "env": format!("P_{}", path.to_uppercase().replace('.', "__")),
        "text_form": "text",
        "required": false,
    });
    for (name, value) in extra.as_object().expect("an object") {
        held[name] = value.clone();
    }
    held
}

/// A key on its own, which is what most of these compare.
fn one(path: &str, extra: &Json) -> Json {
    json!([key(path, extra)])
}

/// The empty half.
fn none() -> Json {
    json!([])
}

fn changes(old: &Json, new: &Json) -> Vec<super::Change> {
    diff_contract("c", "app", "p", Some(&vendored(old)), Some(&vendored(new))).changes
}

#[test]
fn an_absent_side_is_an_ordinary_outcome_and_is_graded() {
    let new = vendored(&document(&none(), &none()));
    let gained = diff_contract("c", "app", "p", None, Some(&new));
    assert_eq!(gained.status, Status::Added);
    assert_eq!(gained.impact(), Severity::Minor);

    let lost = diff_contract("c", "app", "p", Some(&new), None);
    assert_eq!(lost.status, Status::Removed);
    assert_eq!(lost.impact(), Severity::Major);
}

/// A document whose image declares `reload`, or nothing when `reload` is null.
fn reloading(keys: &Json, reload: &Json) -> Json {
    let mut held = document(keys, &none());
    if !reload.is_null() {
        held["schema"]["reload"] = reload.clone();
    }
    held
}

fn rebuild() -> Json {
    json!({"mode": "rebuild", "layers": ["document", "secrets_dir", "env_file"]})
}

fn reload_changes(old: &Json, new: &Json) -> Vec<super::Change> {
    changes(old, new)
        .into_iter()
        .filter(|change| change.area == Area::Reload)
        .collect()
}

#[test]
fn a_class_spelled_differently_but_meaning_the_same_is_not_a_finding() {
    // Undeclared and `restart` roll the same pods, and so do no declaration and `none`.
    let undeclared = reloading(&one("a", &json!({})), &Json::Null);
    let stated = reloading(
        &one("a", &json!({"reload": "restart"})),
        &json!({"mode": "none", "layers": []}),
    );
    assert_eq!(reload_changes(&undeclared, &stated), Vec::new());
    assert!(
        changes(&undeclared, &stated)
            .iter()
            .all(|change| { change.field.as_deref() != Some("reload") })
    );
}

#[test]
fn a_key_changing_class_is_minor_in_both_directions() {
    let restart = reloading(&one("a", &json!({"reload": "restart"})), &rebuild());
    let live = reloading(&one("a", &json!({"reload": "live"})), &rebuild());

    for (old, new, says) in [
        (&restart, &live, "applied by a rebuild"),
        (&live, &restart, "now needs a restart"),
    ] {
        let found = reload_changes(old, new);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].severity, Severity::Minor);
        assert_eq!(found[0].subject, "a");
        assert!(found[0].message.contains(says), "{found:?}");
    }
}

#[test]
fn a_declaration_change_is_one_finding_rather_than_one_per_key() {
    let keys = json!([
        key("a", &json!({"reload": "live"})),
        key("b", &json!({"reload": "live"}))
    ]);
    let before = reloading(&keys, &rebuild());
    let after = reloading(&keys, &json!({"mode": "none", "layers": []}));
    let found = reload_changes(&before, &after);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].subject, "schema.reload");
    assert!(found[0].message.contains("no longer"), "{found:?}");
}

#[test]
fn a_removed_key_is_always_major_because_nothing_can_absorb_it() {
    let found = changes(
        &document(&one("a.b", &json!({})), &none()),
        &document(&none(), &none()),
    );
    let removal = found
        .iter()
        .find(|change| change.kind == Kind::Removed && change.area == Area::Key)
        .expect("the key is gone");
    assert_eq!(removal.severity, Severity::Major);
    assert!(removal.message.contains("refused at boot"), "{removal:?}");
}

#[test]
fn a_removed_external_is_graded_by_what_the_classification_does_next() {
    // With a surviving pattern absorbing it, the removal costs nothing.
    let mut before = document(&none(), &json!([{"name": "RUST_LOG", "text_form": "text"}]));
    before["external"]["ignore"] = json!(["RUST_*"]);
    let mut after = document(&none(), &none());
    after["external"]["ignore"] = json!(["RUST_*"]);
    let found = changes(&before, &after);
    let removal = found
        .iter()
        .find(|change| change.area == Area::External)
        .expect("the variable is gone");
    assert_eq!(removal.severity, Severity::Minor);
    assert!(removal.message.contains("still absorbs it"), "{removal:?}");

    // With none, a chart still setting it stops booting.
    let found = changes(
        &document(&none(), &json!([{"name": "RUST_LOG", "text_form": "text"}])),
        &document(&none(), &none()),
    );
    let removal = found
        .iter()
        .find(|change| change.area == Area::External)
        .expect("the variable is gone");
    assert_eq!(removal.severity, Severity::Major);
}

#[test]
fn an_added_key_is_minor_unless_it_is_required() {
    let optional = changes(
        &document(&none(), &none()),
        &document(&one("a.b", &json!({})), &none()),
    );
    assert_eq!(optional[0].severity, Severity::Minor);

    let required = changes(
        &document(&none(), &none()),
        &document(&one("a.b", &json!({"required": true})), &none()),
    );
    assert_eq!(required[0].severity, Severity::Major);
    assert!(
        required[0].message.contains("will not start"),
        "{required:?}"
    );
}

#[test]
fn a_dialect_change_is_the_most_severe_thing_here() {
    let mut after = document(&none(), &none());
    after["schema"]["dialect"]["prefix"] = json!("Q_");
    let found = changes(&document(&none(), &none()), &after);
    assert_eq!(found[0].severity, Severity::Major);
    assert!(
        found[0]
            .message
            .contains("none of them renders differently"),
        "{found:?}"
    );
}

#[test]
fn a_key_that_stops_being_file_supplyable_is_major_rather_than_minor() {
    // Still spelled the same, still rendered the same, and the mounted secret stops loading.
    let found = changes(
        &document(&one("a.b", &json!({"text_form": "text"})), &none()),
        &document(&one("a.b", &json!({"text_form": "integer"})), &none()),
    );
    let moved = found
        .iter()
        .find(|change| change.field.as_deref() == Some("text_form"))
        .expect("the form moved");
    assert_eq!(moved.severity, Severity::Major);
    assert!(moved.message.contains("mounted file"), "{moved:?}");
}

#[test]
fn becoming_required_is_major_and_ceasing_to_be_is_not() {
    let gained = changes(
        &document(&one("a.b", &json!({})), &none()),
        &document(&one("a.b", &json!({"required": true})), &none()),
    );
    assert_eq!(gained[0].severity, Severity::Major);

    let lost = changes(
        &document(&one("a.b", &json!({"required": true})), &none()),
        &document(&one("a.b", &json!({})), &none()),
    );
    assert_eq!(lost[0].severity, Severity::Minor);
}

#[test]
fn gaining_a_required_entry_is_major_and_losing_one_is_a_relaxation() {
    let map = |required: &[&str]| {
        let mut constraint = json!({"type": "object", "additionalProperties": {"type": "string"}});
        if !required.is_empty() {
            constraint["required"] = json!(required);
        }
        document(
            &one("legal.documents", &json!({"constraint": constraint})),
            &none(),
        )
    };

    let gained = changes(&map(&[]), &map(&["imprint", "privacy"]));
    assert_eq!(
        gained.len(),
        1,
        "one finding, not a flat constraint line beside it: {gained:?}"
    );
    assert_eq!(gained[0].severity, Severity::Major);
    assert_eq!(gained[0].field.as_deref(), Some("constraint.required"));
    assert!(
        gained[0]
            .message
            .contains("must now contain imprint, privacy"),
        "{gained:?}"
    );

    let lost = changes(&map(&["imprint", "privacy"]), &map(&["imprint"]));
    assert_eq!(lost.len(), 1, "{lost:?}");
    assert_eq!(lost[0].severity, Severity::Minor);
    assert!(
        lost[0].message.contains("no longer has to contain privacy"),
        "{lost:?}"
    );

    // Anything else about the constraint moving is still its own finding.
    let mut widened = map(&["imprint"]);
    widened["schema"]["keys"][0]["constraint"]["additionalProperties"] = json!({"type": "integer"});
    let moved = changes(&map(&["imprint"]), &widened);
    assert!(
        moved
            .iter()
            .any(|change| change.field.as_deref() == Some("constraint")),
        "{moved:?}"
    );
}

#[test]
fn gaining_or_replacing_an_entry_name_pattern_is_major_and_losing_one_is_a_relaxation() {
    let map = |pattern: Option<&str>| {
        let mut constraint = json!({"type": "object", "additionalProperties": {"type": "string"}});
        if let Some(pattern) = pattern {
            constraint["propertyNames"] = json!({"pattern": pattern});
        }
        document(
            &one("legal.documents", &json!({"constraint": constraint})),
            &none(),
        )
    };

    let gained = changes(&map(None), &map(Some("^[a-z]+$")));
    assert_eq!(
        gained.len(),
        1,
        "one finding, not a flat constraint line: {gained:?}"
    );
    assert_eq!(gained[0].severity, Severity::Major);
    assert_eq!(gained[0].field.as_deref(), Some("constraint.propertyNames"));
    assert!(
        gained[0]
            .message
            .contains("now holds its entry names to ^[a-z]+$"),
        "{gained:?}"
    );

    // Two patterns cannot be compared for which admits more, so a new one is a tightening.
    let replaced = changes(&map(Some("^[a-z]+$")), &map(Some("^[a-z0-9]+$")));
    assert_eq!(replaced.len(), 1, "{replaced:?}");
    assert_eq!(replaced[0].severity, Severity::Major);

    let lost = changes(&map(Some("^[a-z]+$")), &map(None));
    assert_eq!(lost.len(), 1, "{lost:?}");
    assert_eq!(lost[0].severity, Severity::Minor);
    assert!(
        lost[0]
            .message
            .contains("no longer holds its entry names to ^[a-z]+$"),
        "{lost:?}"
    );

    assert_eq!(
        changes(&map(Some("^[a-z]+$")), &map(Some("^[a-z]+$"))),
        Vec::new()
    );
}

#[test]
fn a_tightening_inside_the_key_is_graded_by_direction_at_its_position() {
    let book = |body: Json| {
        let constraint = json!({"type": "object", "additionalProperties": {
                "type": "object", "properties": {"body": body}}});
        document(
            &one("chapters", &json!({"constraint": constraint})),
            &none(),
        )
    };
    let plain = json!({"type": "object", "additionalProperties": {"type": "string"}});
    let mut matched = plain.clone();
    matched["additionalProperties"]["pattern"] = json!("[^ ]");

    let gained = changes(&book(plain.clone()), &book(matched.clone()));
    assert_eq!(
        gained.len(),
        1,
        "one finding, not a flat constraint line: {gained:?}"
    );
    assert_eq!(gained[0].severity, Severity::Major);
    assert_eq!(
        gained[0].field.as_deref(),
        Some("constraint.*.body.*.pattern")
    );
    assert!(
        gained[0]
            .message
            .contains("at `*.body.*` now has to match [^ ]"),
        "{gained:?}"
    );

    let lost = changes(&book(matched), &book(plain));
    assert_eq!(lost.len(), 1, "{lost:?}");
    assert_eq!(lost[0].severity, Severity::Minor);
    assert!(
        lost[0].message.contains("no longer has to match [^ ]"),
        "{lost:?}"
    );
}

#[test]
fn a_condition_gained_is_major_and_one_lost_is_a_relaxation() {
    let documents = |conditions: Json| {
        let constraint = json!({"type": "object", "additionalProperties": {
                "type": "object", "properties": {"url": {"type": "string"}},
                "allOf": conditions}});
        document(
            &one("documents", &json!({"constraint": constraint})),
            &none(),
        )
    };
    let rule = json!({"description": "`url` is set", "required": ["url"]});

    let gained = changes(&documents(json!([])), &documents(json!([rule.clone()])));
    assert_eq!(gained.len(), 1, "{gained:?}");
    assert_eq!(gained[0].severity, Severity::Major);
    assert_eq!(gained[0].field.as_deref(), Some("constraint.*.allOf"));
    assert!(
        gained[0]
            .message
            .contains("at `*` now has to satisfy: `url` is set"),
        "{gained:?}"
    );

    let lost = changes(&documents(json!([rule])), &documents(json!([])));
    assert_eq!(lost.len(), 1, "{lost:?}");
    assert_eq!(lost[0].severity, Severity::Minor);
}

#[test]
fn a_refinement_that_turns_a_default_into_a_requirement_is_major() {
    let before = document(
        &one(
            "legal.documents",
            &json!({"constraint": {"type": "object"}, "default": "{}", "default_value": {}}),
        ),
        &none(),
    );
    let after = document(
        &one(
            "legal.documents",
            &json!({"constraint": {"type": "object", "required": ["imprint"]}, "required": true}),
        ),
        &none(),
    );
    let found = changes(&before, &after);
    assert_eq!(
        super::worst(found.iter().map(|change| change.severity)),
        Severity::Major
    );
    assert!(
        found
            .iter()
            .any(|change| change.field.as_deref() == Some("required"))
    );
    assert!(
        found
            .iter()
            .any(|change| change.field.as_deref() == Some("constraint.required"))
    );
}

#[test]
fn a_schema_version_rise_this_build_reads_is_a_widening() {
    let mut before = document(&none(), &none());
    before["schema"]["schema_version"] = json!(1);
    let found = changes(&before, &document(&none(), &none()));
    assert_eq!(found[0].severity, Severity::Minor);
    assert!(found[0].message.contains("a widening"), "{found:?}");
}

#[test]
fn a_schema_version_beyond_this_build_is_not() {
    let mut after = document(&none(), &none());
    after["schema"]["schema_version"] = json!(99);
    let found = changes(&document(&none(), &none()), &after);
    assert_eq!(found[0].severity, Severity::Major);
}

#[test]
fn tightening_the_unknown_policy_breaks_a_running_deployment_and_relaxing_it_cannot() {
    let mut lenient = document(&none(), &none());
    lenient["external"]["unknown"] = json!("warn");
    assert_eq!(
        changes(&lenient, &document(&none(), &none()))[0].severity,
        Severity::Major
    );
    assert_eq!(
        changes(&document(&none(), &none()), &lenient)[0].severity,
        Severity::Minor
    );
}

#[test]
fn the_default_and_its_parsed_form_are_one_finding() {
    let found = changes(
        &document(
            &one("a.b", &json!({"default": "1", "default_value": 1})),
            &none(),
        ),
        &document(
            &one("a.b", &json!({"default": "2", "default_value": 2})),
            &none(),
        ),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].field.as_deref(), Some("default"));
    assert_eq!(found[0].severity, Severity::Minor);
}

#[test]
fn a_field_this_diff_models_no_severity_for_is_reported_rather_than_dropped() {
    let found = changes(
        &document(&one("a.b", &json!({"something_later": 1})), &none()),
        &document(&one("a.b", &json!({"something_later": 2})), &none()),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].severity, Severity::Patch);
    assert!(found[0].message.contains("models no severity"), "{found:?}");
}

#[test]
fn an_unreadable_envelope_is_described_rather_than_refused() {
    // The one document this most needs to describe is the one a gate would decline to read.
    let mut after = document(&none(), &none());
    after["terrace_contract"] = json!(99);
    let found = changes(&document(&none(), &none()), &after);
    assert_eq!(found[0].severity, Severity::Major);
    assert_eq!(found[0].subject, "terrace_contract");
}

#[test]
fn the_json_schema_is_only_worth_a_line_when_no_key_moved() {
    let mut after = document(&none(), &none());
    after["json_schema"] = json!({"type": "object"});
    let found = changes(&document(&none(), &none()), &after);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].subject, "json_schema");

    // With a key moving too, the schema is derived from it and saying so again would be noise.
    let mut after = document(&one("a.b", &json!({})), &none());
    after["json_schema"] = json!({"type": "object"});
    let found = changes(&document(&none(), &none()), &after);
    assert!(
        !found.iter().any(|change| change.subject == "json_schema"),
        "{found:?}"
    );
}

#[test]
fn a_message_stays_one_line_when_the_prose_it_quotes_does_not() {
    // A key's documentation is paragraphs, and a finding is a line. The quoting is what keeps
    // that true, and getting it wrong is invisible until a report is read.
    let found = changes(
        &document(&one("a.b", &json!({"docs": "one"})), &none()),
        &document(&one("a.b", &json!({"docs": "one\n\ntwo"})), &none()),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(!found[0].message.contains('\n'), "{:?}", found[0].message);
}

#[test]
fn a_version_this_cannot_read_produces_no_suggestion_rather_than_a_wrong_one() {
    assert_eq!(parse_version(Some("1.2.3")), Some((1, 2, 3)));
    assert_eq!(parse_version(Some("1.2")), None);
    assert_eq!(parse_version(Some("1.2.3-rc1")), None);
    assert_eq!(suggest_version(Some("nope"), Severity::Major), None);
}

#[test]
fn a_suggestion_moves_the_component_the_impact_names() {
    assert_eq!(
        suggest_version(Some("1.2.3"), Severity::Major).as_deref(),
        Some("2.0.0")
    );
    assert_eq!(
        suggest_version(Some("1.2.3"), Severity::Minor).as_deref(),
        Some("1.3.0")
    );
    assert_eq!(
        suggest_version(Some("1.2.3"), Severity::Patch).as_deref(),
        Some("1.2.4")
    );
    assert_eq!(suggest_version(Some("1.2.3"), Severity::None), None);
}

#[test]
fn the_bump_already_in_the_branch_is_read_from_the_pair() {
    assert_eq!(
        observed_bump(Some("1.2.3"), Some("2.0.0")),
        Some(Severity::Major)
    );
    assert_eq!(
        observed_bump(Some("1.2.3"), Some("1.3.0")),
        Some(Severity::Minor)
    );
    assert_eq!(
        observed_bump(Some("1.2.3"), Some("1.2.3")),
        Some(Severity::None)
    );
    assert_eq!(observed_bump(None, Some("1.2.3")), None);
}

#[test]
fn the_impact_is_the_largest_of_the_findings() {
    assert_eq!(
        worst([Severity::Patch, Severity::Major, Severity::Minor]),
        Severity::Major
    );
    assert_eq!(worst([]), Severity::None);
}
