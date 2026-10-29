use serde_json::json;

use super::{
    Added, Departure, Divergence, apply_divergences, covers, expected, ordinal, parse_markers,
    render, scalar,
};

#[test]
fn a_string_with_a_line_break_stays_on_one_line_and_reads_back_the_same() {
    let text = "one\ntwo\r\tthree \"q\" \\ \u{7}\u{85}\u{2028}\u{2029}end";
    let written = scalar(&json!(text));
    assert_eq!(written.lines().count(), 1, "{written:?}");
    assert!(!written.contains(['\u{85}', '\u{2028}', '\u{2029}', '\u{7}']));
    let read = crate::helm::declaration::read_yaml(
        &format!("value: {written}\n"),
        std::path::Path::new("values.yaml"),
    )
    .expect("the scalar parses");
    assert_eq!(read["value"], json!(text));

    // And in a comment, where a raw newline would turn the rest of a default into YAML.
    let rendered = render(&json!({"default": "a\nb: c"}), 0, &Added::default());
    assert_eq!(rendered, ["# default: \"a\\nb: c\""]);
}

fn divergence(sub_path: &str, kind: Departure) -> Divergence {
    Divergence {
        chart: "x".to_owned(),
        line: 1,
        values_path: "a".to_owned(),
        sub_path: sub_path.to_owned(),
        reason: "because".to_owned(),
        kind,
    }
}

#[test]
fn an_enum_is_the_whole_block() {
    // A generator refuses `enum` beside `type` and exits fatally, leaving every chart in the
    // repository without a schema.
    let held = expected(
        Some(&json!({"type": "string", "enum": ["a", "b"]})),
        false,
        false,
    );
    assert_eq!(held, json!({"enum": ["a", "b"]}));
}

#[test]
fn an_optional_enum_gains_null_as_a_member() {
    let held = expected(Some(&json!({"enum": ["a"]})), true, false);
    assert_eq!(held, json!({"enum": ["a", null]}));
}

#[test]
fn a_structured_key_with_no_element_is_opened_rather_than_described() {
    let held = expected(Some(&json!({"type": "object"})), false, true);
    assert_eq!(
        held,
        json!({"type": "object", "additionalProperties": true})
    );
}

#[test]
fn a_structured_key_whose_element_is_described_takes_the_description() {
    // The whole of what `schema_version: 2` buys: the same field, carrying a schema rather
    // than `true`.
    let held = expected(
        Some(&json!({
            "type": "object",
            "additionalProperties": {"type": "object", "properties": {"token": {"type": "string"}}},
        })),
        false,
        true,
    );
    assert_eq!(
        held["additionalProperties"]["properties"]["token"],
        json!({"type": "string"})
    );
}

#[test]
fn a_map_keeps_the_entries_and_names_its_contract_requires() {
    // A refined map: the entries its host refuses to start without, and the names it accepts,
    // travel into the chart's
    // values schema, so an operator's editor and `helm lint` flag the omission before any
    // gate runs. Both the structured path and the plain one carry them.
    let constraint = json!({
        "type": "object",
        "additionalProperties": {"type": "object", "properties": {"title": {"type": "string"}}},
        "required": ["imprint", "privacy"],
        "propertyNames": {"pattern": "^[a-z]+$"},
    });
    for structured in [true, false] {
        let held = expected(Some(&constraint), true, structured);
        assert_eq!(held["required"], json!(["imprint", "privacy"]), "{held}");
        assert_eq!(
            held["propertyNames"],
            json!({"pattern": "^[a-z]+$"}),
            "{held}"
        );
        assert_eq!(
            held["additionalProperties"]["properties"]["title"],
            json!({"type": "string"})
        );
    }
}

#[test]
fn a_structured_key_whose_constraint_names_an_array_is_neither() {
    // Describing it as an object made a chart reject its own defaults the moment its schema
    // was generated.
    let held = expected(
        Some(&json!({"type": "array", "items": {"type": "string"}})),
        false,
        true,
    );
    assert_eq!(held["type"], json!("array"));
    assert_eq!(held["items"], json!({"type": "string"}));
}

#[test]
fn a_constraint_with_no_type_accepts_every_json_type_rather_than_a_guess() {
    let held = expected(Some(&json!({"minimum": 1})), false, false);
    assert_eq!(
        held["type"],
        json!(["string", "integer", "boolean", "array", "object", "null"])
    );
}

#[test]
fn an_optional_value_accepts_null() {
    let held = expected(Some(&json!({"type": "string"})), true, false);
    assert_eq!(held["type"], json!(["string", "null"]));
}

#[test]
fn annotations_do_not_come_across_but_format_does() {
    let held = expected(
        Some(&json!({
            "type": "array",
            "items": {"type": "string", "description": "prose", "format": "uri"},
        })),
        false,
        false,
    );
    assert_eq!(held["items"], json!({"type": "string", "format": "uri"}));
}

#[test]
fn a_struct_field_called_description_is_a_field_and_not_an_annotation() {
    let held = expected(
        Some(&json!({
            "type": "object",
            "properties": {"description": {"type": "string"}},
        })),
        false,
        true,
    );
    assert_eq!(held["properties"]["description"], json!({"type": "string"}));
}

#[test]
fn a_release_is_ordered_numerically_rather_than_as_text() {
    assert!(ordinal("8.10.0") > ordinal("8.9.1"));
    assert!(covers("v8.9.1", "8.9.1"));
    assert!(covers("8.9.1", "8.9.0"));
    assert!(!covers("8.9.0", "8.9.1"));
    // A tag that is not a release is held to equality, which errs towards asking for a re-read.
    assert!(!covers("nightly", "8.9.1"));
    assert!(covers("nightly", "nightly"));
}

#[test]
fn an_override_must_name_a_position_the_contract_describes() {
    let generated = json!({"type": "array", "items": {"properties": {"id": {"type": "integer"}}}});
    let present = json!({"type": "array", "items": {"properties": {"id": {"type": "string"}}}});
    let kept = divergence("items.properties.id", Departure::Override);
    let (result, _added, problems) = apply_divergences(&generated, &present, &[&kept]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        result["items"]["properties"]["id"],
        json!({"type": "string"})
    );

    let wrong = divergence("items.properties.id", Departure::Narrowing);
    let (_, _added, problems) = apply_divergences(&generated, &present, &[&wrong]);
    assert!(
        problems[0].contains("the contract now describes itself"),
        "{problems:?}"
    );
}

#[test]
fn a_narrowing_must_name_a_position_the_contract_does_not_describe() {
    let generated = json!({"type": "object", "properties": {"a": {"type": "number"}}});
    let present = json!({
        "type": "object",
        "properties": {"a": {"type": "number"}, "b": {"type": "string"}},
    });
    let kept = divergence("properties.b", Departure::Narrowing);
    let (result, _added, problems) = apply_divergences(&generated, &present, &[&kept]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(result["properties"]["b"], json!({"type": "string"}));

    let wrong = divergence("properties.a", Departure::Narrowing);
    let (_, _added, problems) = apply_divergences(&generated, &present, &[&wrong]);
    assert!(problems[0].contains("now describes itself"), "{problems:?}");
}

#[test]
fn a_departure_naming_nothing_in_the_block_has_nothing_to_keep() {
    let kept = divergence("properties.nope", Departure::Narrowing);
    let (_, _added, problems) = apply_divergences(&json!({}), &json!({}), &[&kept]);
    assert!(problems[0].contains("nothing to keep"), "{problems:?}");
}

#[test]
fn a_narrowing_that_reintroduces_an_enum_beside_a_type_settles_on_the_enum() {
    // A generator exits fatally on the pair, so the rule is applied after the departures as
    // well as before them.
    let generated = json!({"type": "string"});
    let present = json!({"type": "string", "enum": ["debug", "info"]});
    let kept = divergence("enum", Departure::Narrowing);
    let (result, _added, problems) = apply_divergences(&generated, &present, &[&kept]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(result, json!({"enum": ["debug", "info"]}));
}

#[test]
fn the_rendering_is_block_form_and_keeps_the_canonical_order() {
    let rendered = render(
        &json!({"minimum": 1, "type": "integer", "maximum": 9}),
        2,
        &Added::default(),
    );
    assert_eq!(
        rendered,
        ["  # type: integer", "  # minimum: 1", "  # maximum: 9"]
    );
}

#[test]
fn the_null_type_is_quoted_and_a_null_enum_member_is_not() {
    assert_eq!(
        render(&json!({"type": ["string", "null"]}), 0, &Added::default()),
        ["# type: [string, 'null']"]
    );
    assert_eq!(
        render(&json!({"enum": ["a", null]}), 0, &Added::default()),
        ["# enum: [\"a\", null]"]
    );
}

#[test]
fn a_field_yaml_would_read_as_a_boolean_is_quoted() {
    let rendered = render(
        &json!({"type": "object", "properties": {"on": {"type": "string"}}}),
        0,
        &Added::default(),
    );
    assert!(
        rendered.iter().any(|line| line.contains("\"on\":")),
        "{rendered:?}"
    );
}

#[test]
fn a_struct_field_named_like_a_keyword_is_still_a_field() {
    let rendered = render(
        &json!({"type": "object", "properties": {"items": {"type": "string"}}}),
        0,
        &Added::default(),
    );
    assert_eq!(
        rendered,
        [
            "# type: object",
            "# properties:",
            "#   items:",
            "#     type: string",
        ]
    );
}

#[test]
fn the_retired_enrolment_marker_is_refused_by_name() {
    let error = parse_markers("# @config-shape a.b generated\n", "x")
        .expect_err("the obsolete spelling is refused");
    assert!(error.to_string().contains("is obsolete"), "{error}");
}

#[test]
fn a_transcription_names_the_release_and_the_source() {
    let (shapes, _) = parse_markers("# @config-shape a.b handwritten 8.9.1 src/config.rs\n", "x")
        .expect("the marker reads");
    assert_eq!(shapes.len(), 1);
    assert_eq!(shapes[0].values_path, "a.b");
    assert!(shapes[0].handwritten);
    assert_eq!(shapes[0].version.as_deref(), Some("8.9.1"));
    assert_eq!(shapes[0].source.as_deref(), Some("src/config.rs"));

    assert!(
        parse_markers("# @config-shape a.b handwritten 8.9.1\n", "x")
            .expect_err("a transcription names both")
            .to_string()
            .contains("takes the release")
    );
}

#[test]
fn a_departure_without_a_reason_is_refused() {
    let error = parse_markers("# @config-shape-except a.b items.id\n", "x")
        .expect_err("a departure carries a reason");
    assert!(error.to_string().contains("nobody can review"), "{error}");
}

#[test]
fn a_marker_named_in_prose_is_not_a_declaration() {
    // Anchored as a whole comment line, so this file's own documentation and a chart's `# --`
    // description do not declare anything.
    let (shapes, divergences) = parse_markers(
        "# -- see @config-shape a.b handwritten 1 x for the escape hatch\n",
        "x",
    )
    .expect("prose reads");
    assert!(shapes.is_empty() && divergences.is_empty());
}
