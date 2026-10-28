//! The planner and the renderers, over hand-built contract keys rather than a registry.
//!
//! Ported from the scaffold's own suite, for the part of it this module carries: naming,
//! placement, the value and `@schema` block one key becomes, the template lines, and the
//! write-off groups. The cases about a whole rendered chart stay with the renderers that are
//! not ported yet.

use std::collections::BTreeMap;

use serde_json::{Map, Value as Json, json};

use super::{
    Destination, Key, Node, Placement, Plan, check_chart_name, credential_entries_for, default_for,
    derived_for, env_prefix, invented, plan_keys, schema_lines, service_key_for, tree_at,
    value_block, values_path_for, write_off_groups,
};
use crate::helm::declaration::read_yaml;
use crate::union::Merged;

/// One contract key, with the fields every consumer reads, and `overrides` laid over them.
fn key(path: &str, overrides: &Json) -> Key {
    let spelt = path.replace('.', "__");
    let mut fields: Map<String, Json> = json!({
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
    })
    .as_object()
    .expect("an object")
    .clone();
    for (name, value) in overrides.as_object().expect("overrides are an object") {
        fields.insert(name.clone(), value.clone());
    }
    Key::from(Merged {
        name: path.to_owned(),
        fields,
        sources: vec!["test/app.json".to_owned()],
    })
}

fn plan_of(keys: Vec<Key>) -> Plan {
    plan_keys(keys).expect("the keys plan")
}

fn paths(placements: &[Placement]) -> Vec<&str> {
    placements.iter().map(Placement::path).collect()
}

/// The `@schema` block one key's value carries, read the way helm-schema reads it: the text
/// between the delimiters is YAML, and the marker inside it is a YAML comment.
fn block(entry: &Key) -> Json {
    let body: Vec<String> = schema_lines(&entry.entry, "")
        .iter()
        .map(|line| {
            let comment = line.strip_prefix('#').unwrap_or(line);
            comment.strip_prefix(' ').unwrap_or(comment).to_owned()
        })
        .collect();
    read_yaml(&body.join("\n"), std::path::Path::new("block")).expect("the block is YAML")
}

fn only(plan: &Plan) -> &Placement {
    plan.surfaced().next().expect("one surfaced key")
}

// --------------------------------------------------------------------------------------
// Naming
// --------------------------------------------------------------------------------------

#[test]
fn each_segment_is_camel_cased() {
    assert_eq!(
        values_path_for("feed.check_interval_secs"),
        "feed.checkIntervalSecs"
    );
    assert_eq!(values_path_for("discord.webhook_url"), "discord.webhookUrl");
}

#[test]
fn a_segment_without_an_underscore_is_untouched() {
    assert_eq!(values_path_for("metrics.ip"), "metrics.ip");
}

#[test]
fn a_service_is_read_from_an_image_path_of_the_service_shape_alone() {
    assert_eq!(
        service_key_for("services.worker.image").as_deref(),
        Some("worker")
    );
    assert_eq!(service_key_for("image"), None);
    assert_eq!(service_key_for("services.worker.sidecar.image"), None);
}

#[test]
fn the_env_prefix_drops_the_separator_the_partial_appends() {
    // `common.fileConfig.env` appends the separator itself, so passing the contract's own
    // spelling through would produce `APP__CONFIG`.
    assert_eq!(env_prefix("APP_"), "APP");
    assert_eq!(env_prefix("NETCUP_OFFER_BOT_"), "NETCUP_OFFER_BOT");
}

#[test]
fn a_name_helm_would_refuse_is_refused_here() {
    for name in ["Foo", "foo_bar", "-foo", "foo-", "", "foo\n"] {
        assert!(check_chart_name(name).is_err(), "{name:?} was accepted");
    }
    for name in ["foo-bar-1", "a", "0"] {
        check_chart_name(name).unwrap_or_else(|failure| panic!("{name:?}: {failure}"));
    }
}

#[test]
fn a_name_past_helms_release_limit_is_refused_at_the_limit_and_not_before() {
    check_chart_name(&"a".repeat(53)).expect("53 is Helm's limit, and allowed");
    let error = check_chart_name(&"a".repeat(54)).expect_err("54 is one past it");
    assert!(error.to_string().contains("at most 53"), "{error}");
}

// --------------------------------------------------------------------------------------
// Placement
// --------------------------------------------------------------------------------------

#[test]
fn a_file_supplyable_credential_becomes_a_secret_file() {
    let plan = plan_of(vec![key("a.token", &json!({"secret": true}))]);
    assert_eq!(paths(&plan.credentials), ["a.token"]);
    assert_eq!(plan.projected, [] as [Placement; 0]);
    assert_eq!(plan.credentials[0].destination, Destination::SecretFile);
}

#[test]
fn a_credential_no_file_can_supply_is_written_off() {
    let plan = plan_of(vec![key(
        "a.port",
        &json!({"secret": true, "text_form": "integer", "constraint": {"type": "integer"}}),
    )]);
    assert_eq!(paths(&plan.written_off), ["a.port"]);
    assert_eq!(plan.credentials, [] as [Placement; 0]);
    assert!(
        plan.written_off[0]
            .reason
            .contains("whose `text_form` is 'integer'")
    );
}

#[test]
fn a_reserved_key_is_written_off() {
    let plan = plan_of(vec![key("a.b", &json!({"reserved": true}))]);
    assert_eq!(paths(&plan.written_off), ["a.b"]);
    assert_eq!(plan.written_off[0].values_path, "");
}

#[test]
fn every_written_off_key_carries_a_reason() {
    let plan = plan_of(vec![
        key("a.b", &json!({"reserved": true})),
        key(
            "c.d",
            &json!({"secret": true, "text_form": "integer", "constraint": {"type": "integer"}}),
        ),
    ]);
    assert_eq!(plan.written_off.len(), 2);
    for item in &plan.written_off {
        assert!(!item.reason.trim().is_empty(), "{}", item.path());
    }
}

#[test]
fn keys_are_planned_in_path_order_whatever_order_they_came_in() {
    let plan = plan_of(vec![key("b.x", &json!({})), key("a.y", &json!({}))]);
    assert_eq!(paths(&plan.projected), ["a.y", "b.x"]);
}

// --------------------------------------------------------------------------------------
// The values tree
// --------------------------------------------------------------------------------------

#[test]
fn a_key_whose_parent_is_another_key_is_refused() {
    // `a` cannot be both a scalar value and the map `a.b` lives in. Refused rather than
    // rendered as whichever YAML happens to win — in either order.
    let plan = plan_of(vec![key("a", &json!({})), key("a.b", &json!({}))]);
    let error = tree_at(&plan.projected, 0).expect_err("the two cannot both exist");
    assert!(
        error
            .to_string()
            .contains("is already a leaf value on the way there")
    );

    let reversed: Vec<Placement> = plan.projected.iter().rev().cloned().collect();
    assert!(tree_at(&reversed, 0).is_err());
}

#[test]
fn two_keys_spelling_one_chart_value_are_refused() {
    let plan = plan_of(vec![key("a.b_c", &json!({})), key("a.bC", &json!({}))]);
    let error = tree_at(&plan.projected, 0).expect_err("one value cannot feed two keys");
    assert!(
        error
            .to_string()
            .contains("which another key already needs")
    );
}

#[test]
fn a_setting_and_a_credential_under_one_prefix_share_one_block() {
    let plan = plan_of(vec![
        key("telemetry.log_level", &json!({})),
        key("telemetry.sentry_dsn", &json!({"secret": true})),
    ]);
    let surfaced: Vec<Placement> = plan.surfaced().cloned().collect();
    let tree = tree_at(&surfaced, 0).expect("the tree builds");
    let Some(Node::Branch(telemetry)) = tree.get("telemetry") else {
        panic!("one `telemetry` block: {tree:?}");
    };
    assert_eq!(
        telemetry.keys().map(String::as_str).collect::<Vec<_>>(),
        ["logLevel", "sentryDsn"]
    );
}

#[test]
fn the_image_default_is_written_through() {
    let plan = plan_of(vec![key("a.b", &json!({"default_value": "chosen"}))]);
    assert_eq!(default_for(&only(&plan).key), json!("chosen"));
}

#[test]
fn an_optional_key_with_no_default_is_null() {
    let plan = plan_of(vec![key("a.b", &json!({}))]);
    assert_eq!(default_for(&only(&plan).key), Json::Null);
    let lines = value_block(only(&plan), "  ").expect("a block");
    assert_eq!(lines.last().map(String::as_str), Some("  b: null"));
}

#[test]
fn a_required_key_with_no_default_gets_a_value_its_constraint_accepts() {
    let plan = plan_of(vec![key(
        "a.port",
        &json!({
            "required": true,
            "text_form": "integer",
            "constraint": {"type": "integer", "minimum": 8000, "maximum": 9000},
        }),
    )]);
    let value = default_for(&only(&plan).key);
    let number = value.as_i64().expect("an integer");
    assert!((8000..=9000).contains(&number), "{value}");
}

#[test]
fn every_invented_value_is_reported() {
    let plan = plan_of(vec![
        key("a.b", &json!({"required": true})),
        key("c.d", &json!({"required": true, "default_value": "known"})),
    ]);
    assert_eq!(
        invented(&plan)
            .into_iter()
            .map(Placement::path)
            .collect::<Vec<_>>(),
        ["a.b"]
    );
}

#[test]
fn a_credential_carries_no_marker_and_an_empty_value() {
    let plan = plan_of(vec![key("a.token", &json!({"secret": true}))]);
    let lines = value_block(only(&plan), "").expect("a block");
    assert!(
        lines.iter().all(|line| !line.contains("@config")),
        "{lines:?}"
    );
    assert_eq!(lines.last().map(String::as_str), Some("token: \"\""));
    assert!(lines.iter().any(|line| line.contains("`a__token`")));
}

#[test]
fn a_projected_key_carries_an_optional_marker_and_a_required_one_does_not() {
    let plan = plan_of(vec![
        key("a.b", &json!({})),
        key("a.c", &json!({"required": true})),
        key(
            "a.d",
            &json!({"text_form": "structured", "constraint": {"type": "array"}}),
        ),
    ]);
    let marker =
        |index: usize| value_block(&plan.projected[index], "").expect("a block")[1].clone();
    assert_eq!(marker(0), "# # @config projection a.b optional");
    assert_eq!(marker(1), "# # @config projection a.c");
    assert_eq!(marker(2), "# # @config structured a.d optional");
}

// --------------------------------------------------------------------------------------
// The `@schema` block
// --------------------------------------------------------------------------------------

#[test]
fn a_structured_array_keeps_the_constraint_the_contract_published() {
    let entry = key(
        "a.hosts",
        &json!({
            "text_form": "structured",
            "default_value": ["one"],
            "constraint": {"type": "array", "items": {"type": "string"}},
        }),
    );
    let found = block(&entry);
    assert_eq!(found["type"], json!(["array", "null"]));
    assert_eq!(found["items"], json!({"type": "string"}));
    assert!(found.get("additionalProperties").is_none());
}

#[test]
fn a_required_structured_array_defaults_to_an_array_rather_than_a_table() {
    let entry = key(
        "a.hosts",
        &json!({
            "text_form": "structured",
            "required": true,
            "constraint": {"type": "array", "items": {"type": "string"}},
        }),
    );
    assert_eq!(default_for(&entry.entry), json!([]));
}

#[test]
fn a_structured_object_is_still_opened_rather_than_described() {
    let entry = key(
        "a.peers",
        &json!({"text_form": "structured", "constraint": {"type": "object"}}),
    );
    let found = block(&entry);
    assert_eq!(found["type"], json!(["object", "null"]));
    assert_eq!(found["additionalProperties"], json!(true));
}

#[test]
fn a_structured_key_with_no_declared_type_is_opened_too() {
    let entry = key(
        "a.peers",
        &json!({"text_form": "structured", "constraint": {}}),
    );
    let found = block(&entry);
    assert_eq!(found["type"], json!(["object", "null"]));
    assert_eq!(found["additionalProperties"], json!(true));
}

#[test]
fn an_enum_is_the_whole_block() {
    // helm-schema refuses a block carrying both `enum` and `type`, fatally.
    let entry = key(
        "a.backend",
        &json!({
            "text_form": "choice",
            "default_value": "sqlite",
            "constraint": {"type": "string", "enum": ["sqlite", "postgres"]},
        }),
    );
    let found = block(&entry);
    assert!(found.get("type").is_none(), "{found}");
    assert_eq!(found["enum"], json!(["sqlite", "postgres", null]));
}

#[test]
fn a_required_enum_gains_no_null_member() {
    let entry = key(
        "a.backend",
        &json!({
            "text_form": "choice",
            "required": true,
            "default_value": "sqlite",
            "constraint": {"type": "string", "enum": ["sqlite", "postgres"]},
        }),
    );
    assert_eq!(block(&entry)["enum"], json!(["sqlite", "postgres"]));
}

#[test]
fn no_generated_block_carries_both_enum_and_type() {
    for entry in [
        key("a.text", &json!({})),
        key(
            "a.port",
            &json!({"text_form": "integer", "constraint": {"type": "integer", "minimum": 1}}),
        ),
        key(
            "a.backend",
            &json!({
                "text_form": "choice",
                "constraint": {"type": "string", "enum": ["sqlite", "postgres"]},
            }),
        ),
        key(
            "a.hosts",
            &json!({
                "text_form": "structured",
                "constraint": {"type": "array", "items": {"type": "string"}},
            }),
        ),
        key(
            "a.peers",
            &json!({"text_form": "structured", "constraint": {"type": "object"}}),
        ),
        key("a.token", &json!({"secret": true})),
    ] {
        let found = block(&entry);
        assert!(
            !(found.get("enum").is_some() && found.get("type").is_some()),
            "{}: {found}",
            entry.entry.name
        );
    }
}

#[test]
fn a_bounded_scalar_still_copies_its_bounds() {
    let entry = key(
        "a.port",
        &json!({
            "text_form": "integer",
            "constraint": {"type": "integer", "minimum": 1, "maximum": 65535},
        }),
    );
    let found = block(&entry);
    assert_eq!(found["type"], json!(["integer", "null"]));
    assert_eq!(
        (&found["minimum"], &found["maximum"]),
        (&json!(1), &json!(65535))
    );
}

// --------------------------------------------------------------------------------------
// The template lines
// --------------------------------------------------------------------------------------

fn derived(keys: Vec<Key>) -> String {
    let plan = plan_of(keys);
    derived_for(&plan.projected, "", None, ".Values").join("\n")
}

#[test]
fn an_optional_key_is_wrapped_so_an_unset_value_is_absent() {
    assert!(derived(vec![key("a.b", &json!({}))]).contains("{{- with .Values.a.b }}"));
}

#[test]
fn a_required_key_is_written_unconditionally() {
    let text = derived(vec![key("a.b", &json!({"required": true}))]);
    assert!(text.contains("b: {{ .Values.a.b | quote }}"), "{text}");
    assert!(!text.contains("with .Values.a.b"), "{text}");
}

#[test]
fn a_numeric_key_is_not_quoted() {
    let text = derived(vec![key(
        "a.port",
        &json!({"required": true, "text_form": "integer", "constraint": {"type": "integer"}}),
    )]);
    assert!(text.contains("port: {{ .Values.a.port }}"), "{text}");
}

#[test]
fn an_optional_number_or_boolean_is_guarded_against_null_not_falsy() {
    // `with` would drop a deliberate `0` or `false`, which are settings and not "unset" —
    // whether the contract spells the type as a list or as a bare string.
    for (path, constraint) in [
        ("a.count", json!({"type": ["integer", "null"]})),
        ("a.enabled", json!({"type": ["boolean", "null"]})),
        ("a.count", json!({"type": "integer"})),
    ] {
        let text = derived(vec![key(path, &json!({"constraint": constraint}))]);
        let leaf = path.rsplit('.').next().expect("a leaf");
        let guard = format!("{{{{- if not (kindIs \"invalid\" .Values.{path}) }}}}");
        assert!(text.contains(&guard), "{text}");
        assert!(
            text.contains(&format!("{leaf}: {{{{ .Values.{path} }}}}")),
            "{text}"
        );
        assert!(!text.contains(&format!("with .Values.{path}")), "{text}");
    }
}

#[test]
fn a_root_only_some_documents_declare_is_guarded_by_service() {
    let plan = plan_of(vec![key("queue.workers", &json!({"required": true}))]);
    let scoped = BTreeMap::from([("queue".to_owned(), vec!["worker".to_owned()])]);
    let lines = derived_for(&plan.projected, "", Some(&scoped), "$ctx.Values");
    assert_eq!(
        lines.first().map(String::as_str),
        Some("{{- if eq $service \"worker\" }}")
    );
    assert_eq!(lines.last().map(String::as_str), Some("{{- end }}"));
    assert!(lines.contains(&"  workers: {{ $ctx.Values.queue.workers | quote }}".to_owned()));

    let both = BTreeMap::from([(
        "queue".to_owned(),
        vec!["api".to_owned(), "worker".to_owned()],
    )]);
    let lines = derived_for(&plan.projected, "", Some(&both), "$ctx.Values");
    assert_eq!(
        lines[0],
        "{{- if or (eq $service \"api\") (eq $service \"worker\") }}"
    );
}

#[test]
fn every_credential_reaches_the_secret_under_its_published_file_name() {
    let plan = plan_of(vec![key("a.token", &json!({"secret": true}))]);
    assert_eq!(
        credential_entries_for(&plan.credentials).expect("lines"),
        ["a__token: {{ .Values.a.token | quote }}"]
    );
}

#[test]
fn a_credential_with_no_secrets_file_is_refused_rather_than_named() {
    let plan = plan_of(vec![key(
        "a.token",
        &json!({"secret": true, "secrets_file": ""}),
    )]);
    let error = credential_entries_for(&plan.credentials).expect_err("there is no name to use");
    assert!(error.to_string().contains("declares no `secrets_file`"));
}

#[test]
fn the_template_braces_are_balanced() {
    let plan = plan_of(vec![
        key("a.b", &json!({})),
        key("a.n", &json!({"constraint": {"type": "integer"}})),
        key("c.token", &json!({"secret": true, "required": true})),
    ]);
    let mut text = derived_for(&plan.projected, "", None, ".Values").join("\n");
    text.push_str(
        &credential_entries_for(&plan.credentials)
            .expect("lines")
            .join("\n"),
    );
    assert_eq!(
        text.matches("{{").count(),
        text.matches("}}").count(),
        "{text}"
    );
}

// --------------------------------------------------------------------------------------
// The declaration
// --------------------------------------------------------------------------------------

#[test]
fn write_offs_sharing_a_reason_are_one_entry() {
    let plan = plan_of(vec![
        key("a.x", &json!({"reserved": true})),
        key("a.y", &json!({"reserved": true})),
        key("a.z", &json!({})),
    ]);
    let groups = write_off_groups(&plan).expect("groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, ["a.x", "a.y"]);
}

#[test]
fn a_credential_is_written_off_naming_its_file_and_its_value() {
    let plan = plan_of(vec![key("a.token", &json!({"secret": true}))]);
    let groups = write_off_groups(&plan).expect("groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, ["a.token"]);
    assert!(groups[0].1.contains("`a__token`"));
    assert!(
        groups[0]
            .1
            .contains("`a.token` is the value that carries it")
    );
}
