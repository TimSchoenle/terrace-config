//! The whole files, over hand-built contracts rather than a registry.
//!
//! The rest of the scaffold's suite. The cases about one key (its placement, its `@schema` block,
//! its template line) are in [`super::super`]'s own suite; these are about what only a whole file
//! shows: that the values file is one YAML document with every value where the parser expects it,
//! that the markers in it are ones the marker reader accepts, that every block it writes is one the
//! README gate accepts, and that the declaration, its round-trip enrolment included, is one the
//! reader every gate uses accepts.
//!
//! Every assertion is made through the reader that decides it rather than by matching the bytes
//! a renderer happened to write: [`crate::helm::markers::read`] for markers,
//! [`crate::helm::declaration::parse_declaration`] for the declaration and its enrolment, and
//! [`crate::value::assert_value`] for an invented value. Two exceptions are written here because no Rust reader of them exists:
//! [`blocks`], which reads `@schema` blocks the way helm-schema does, and [`undocumented`], which
//! is `check-values-docs`' rule.

use std::path::Path;

use serde_json::{Map, Value as Json, json};

use super::{
    CONFIG_ROOT, Surface, probe_credential, render_ci_values, render_declaration, render_helpers,
    render_values, undescribed,
};
use crate::helm::declaration::{parse_declaration, read_yaml};
use crate::helm::markers::{self, Class, Marker};
use crate::union::{Union, union_contracts};
use crate::value::assert_value;

/// Blocks of the shape a repository's chassis holds, standing in for one: an enum without a
/// type, a grouping block with children, a block with an open object, and an empty mapping.
const CHASSIS: &str = "\
# @schema
# enum: [restricted, none]
# @schema
# -- Pod security context baseline.
podSecurityContextPreset: restricted

# @schema
# additionalProperties: true
# @schema
# -- ServiceAccount the pods run under.
serviceAccount:
  # @schema
  # type: boolean
  # @schema
  # -- Whether to create a dedicated service account
  create: true

# @schema
# additionalProperties: true
# @schema
# -- Restrict the traffic the pods accept.
networkPolicy:
  # @schema
  # type: boolean
  # @schema
  # -- Whether to render a NetworkPolicy
  enabled: false

# @schema
# type: object
# additionalProperties: true
# @schema
# -- Container resource requests and limits.
resources: {}
";

/// One contract key, with the fields every consumer reads, and `overrides` laid over them.
fn key(path: &str, overrides: &Json) -> Json {
    let spelt = path.replace('.', "__");
    let mut fields = json!({
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
    });
    let held = fields.as_object_mut().expect("an object");
    for (name, value) in overrides.as_object().expect("overrides are an object") {
        held.insert(name.clone(), value.clone());
    }
    fields
}

/// A one-contract union carrying these keys and the dialect every contract here declares,
/// through the same merge a fetched contract goes through.
fn union_of(keys: &[Json]) -> Union {
    let contract = json!({
        "terrace_contract": 1,
        "app": {"name": "app", "version": "1.0.0"},
        "schema": {
            "schema_version": 2,
            "dialect": {
                "prefix": "APP_",
                "nesting_separator": "__",
                "indirection_suffix": "_FILE",
            },
            "loader": [],
            "keys": keys,
        },
        "json_schema": {},
        "external": {"env": [], "ignore": [], "unknown": "reject"},
    });
    union_contracts(&[("test/app.json".to_owned(), contract)]).expect("the contract unions")
}

fn surface(keys: &[Json]) -> Surface {
    Surface::from_contract(&union_of(keys)).expect("the keys plan")
}

fn values_text(surface: &Surface, chassis: &str) -> String {
    render_values("app", surface, "org/app", "v1.0.0", chassis, "config.toml")
        .expect("the values render")
}

/// The generated `values.yaml`, parsed. Proves it is YAML as well as what it says.
fn values_of(surface: &Surface) -> Json {
    read_yaml(&values_text(surface, ""), Path::new("values.yaml")).expect("values are YAML")
}

/// Every marker in a generated values file, as the marker gate reads them.
fn markers_of(text: &str) -> Vec<Marker> {
    markers::read(text, "app").expect("the markers read").0
}

fn helpers(keys: &[Json]) -> String {
    render_helpers("app", &surface(keys)).expect("the helpers render")
}

/// Every `@schema` block in a values file, with the key it describes and that key's default.
///
/// Read the way helm-schema reads them: the text between the two delimiters is YAML, and the
/// `# @config` marker inside it is a YAML comment, so this sees exactly what the schema generator
/// will be handed, which is what makes an assertion about one worth anything.
fn blocks(text: &str) -> Vec<(String, Json, Json)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut number = 0;
    while number < lines.len() {
        if lines[number].trim() != "# @schema" {
            number += 1;
            continue;
        }
        let indent = lines[number].len() - lines[number].trim_start().len();
        let mut body: Vec<&str> = Vec::new();
        number += 1;
        while number < lines.len() && lines[number].trim() != "# @schema" {
            let line = lines[number].get(indent..).unwrap_or_default();
            let line = line.strip_prefix('#').unwrap_or(line);
            body.push(line.strip_prefix(' ').unwrap_or(line));
            number += 1;
        }
        // Past the closing delimiter, past the description, onto the key the run belongs to.
        number += 1;
        while number < lines.len() && lines[number].trim_start().starts_with('#') {
            number += 1;
        }
        let line = lines.get(number).map_or("", |line| line.trim());
        let (name, written) = line.split_once(':').unwrap_or((line, ""));
        let parse = |text: &str| read_yaml(text, Path::new("block")).expect("YAML");
        let default = if written.trim().is_empty() {
            Json::Null
        } else {
            parse(written)
        };
        let block = parse(&body.join("\n"));
        found.push((name.to_owned(), block, default));
    }
    found
}

/// The parsed `@schema` block the renderer writes above one key's value.
fn block_for(text: &str, leaf: &str) -> (Json, Json) {
    blocks(text)
        .into_iter()
        .find(|(name, _, _)| name == leaf)
        .map_or_else(
            || panic!("no block for `{leaf}` in\n{text}"),
            |(_, block, default)| (block, default),
        )
}

/// The JSON Schema type name of one rendered default.
fn json_type(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "boolean",
        Json::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Json::Number(_) => "number",
        Json::String(_) => "string",
        Json::Array(_) => "array",
        Json::Object(_) => "object",
    }
}

/// The values `check-values-docs` would report: every mapping key with an `@schema` block or
/// with children, whose contiguous comment run above it holds no `# --` line.
///
/// The gate's own rule, line for line. It lives in the chart repository and reads text, because the
/// description is a comment and a YAML parser discards comments. Restated rather than re-derived: a
/// second reader that agreed with these renderers and disagreed with the gate is the failure the
/// assertion exists to catch.
fn undocumented(text: &str) -> Vec<String> {
    struct Found {
        indent: usize,
        path: String,
        owes: bool,
        described: bool,
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut found: Vec<Found> = Vec::new();
    let mut scalar_indent: Option<usize> = None;
    for (number, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let stripped = line.trim_start();
        let indent = line.len() - stripped.len();
        if let Some(held) = scalar_indent {
            if indent > held {
                continue;
            }
            scalar_indent = None;
        }
        if stripped.starts_with('#') {
            continue;
        }
        let Some((name, rest)) = stripped.split_once(':') else {
            continue;
        };
        let is_key = !name.is_empty()
            && name
                .chars()
                .all(|held| held.is_ascii_alphanumeric() || "_.-/".contains(held))
            && (rest.is_empty() || rest.starts_with(char::is_whitespace));
        if !is_key {
            continue;
        }
        if ["|", ">", "|-", ">-", "|+", ">+"].contains(&rest.trim()) {
            scalar_indent = Some(indent);
        }
        let (mut described, mut schema) = (false, false);
        for above in lines[..number].iter().rev() {
            let comment = above.trim_start();
            if !comment.starts_with('#') {
                break;
            }
            described |= comment.starts_with("# --");
            schema |= comment.starts_with("# @schema");
        }
        found.push(Found {
            indent,
            path: name.to_owned(),
            owes: schema,
            described,
        });
    }
    let mut stack: Vec<usize> = Vec::new();
    for index in 0..found.len() {
        while stack
            .last()
            .is_some_and(|&top| found[top].indent >= found[index].indent)
        {
            stack.pop();
        }
        if let Some(&parent) = stack.last() {
            found[parent].owes = true;
            found[index].path = format!("{}.{}", found[parent].path, found[index].path);
        }
        stack.push(index);
    }
    found
        .into_iter()
        .filter(|key| key.owes && !key.described)
        .map(|key| key.path)
        .collect()
}

/// Each top-level key's `# --` line, keyed by the key it sits above.
fn described(text: &str) -> Map<String, Json> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Map::new();
    for (number, line) in lines.iter().enumerate() {
        if line.starts_with(['#', ' ']) || !line.ends_with(':') {
            continue;
        }
        if let Some(description) = lines[..number]
            .iter()
            .rev()
            .take_while(|above| above.starts_with('#'))
            .find(|above| above.starts_with("# -- "))
        {
            found.insert(
                line.trim_end_matches(':').to_owned(),
                Json::String((*description).to_owned()),
            );
        }
    }
    found
}

// --------------------------------------------------------------------------------------
// Markers
// --------------------------------------------------------------------------------------

#[test]
fn a_projected_key_carries_a_marker_the_parser_accepts() {
    let found = markers_of(&values_text(&surface(&[key("a.b", &json!({}))]), ""));
    assert_eq!(found.len(), 1, "{found:?}");
    let marker = &found[0];
    assert_eq!(
        (marker.class, marker.target.as_str(), marker.optional),
        (Class::Projection, "a.b", true)
    );
    assert_eq!(marker.values_path, "a.b");
    assert_eq!(marker.documents, None);
    assert_eq!(marker.condition, None);
}

#[test]
fn a_required_key_is_not_marked_optional() {
    let found = markers_of(&values_text(
        &surface(&[key("a.b", &json!({"required": true}))]),
        "",
    ));
    assert!(!found[0].optional, "{found:?}");
}

#[test]
fn a_structured_key_is_marked_structured() {
    let found = markers_of(&values_text(
        &surface(&[key(
            "a.b",
            &json!({"text_form": "structured", "constraint": {"type": "array"}}),
        )]),
        "",
    ));
    assert_eq!(found[0].class, Class::Structured);
}

#[test]
fn a_credential_carries_no_marker() {
    // The marker gate refuses a key that is both marked and written off, and a file-delivered
    // credential is written off, so a marker on it would fail the chart's first run.
    let text = values_text(&surface(&[key("a.token", &json!({"secret": true}))]), "");
    assert_eq!(markers_of(&text), Vec::<Marker>::new());
    // ...and the value still exists, because the Secret is rendered from it.
    assert!(text.contains("token:"), "{text}");
}

// --------------------------------------------------------------------------------------
// The values tree
// --------------------------------------------------------------------------------------

#[test]
fn a_setting_and_a_credential_under_one_prefix_share_one_block() {
    // Two sections would emit the prefix twice, and YAML would keep only the second.
    let values = values_of(&surface(&[
        key("telemetry.log_level", &json!({})),
        key("telemetry.sentry_dsn", &json!({"secret": true})),
    ]));
    let names: Vec<&String> = values["telemetry"]
        .as_object()
        .expect("one telemetry block")
        .keys()
        .collect();
    assert_eq!(names, ["logLevel", "sentryDsn"]);
}

#[test]
fn a_key_whose_parent_is_another_key_is_refused() {
    // `a` cannot be both a scalar and the map `a.b` lives in. Refused rather than rendered as
    // whichever YAML happens to win.
    let rendered =
        Surface::from_contract(&union_of(&[key("a", &json!({})), key("a.b", &json!({}))]))
            .and_then(|surface| render_values("app", &surface, "org/app", "v1", "", "config.toml"));
    assert!(rendered.is_err(), "{rendered:?}");
}

#[test]
fn the_image_default_is_written_through() {
    let values = values_of(&surface(&[key("a.b", &json!({"default_value": "chosen"}))]));
    assert_eq!(values["a"]["b"], "chosen");
}

#[test]
fn an_optional_key_with_no_default_is_null() {
    let values = values_of(&surface(&[key("a.b", &json!({}))]));
    assert_eq!(values["a"].get("b"), Some(&Json::Null));
}

#[test]
fn a_required_key_with_no_default_gets_a_value_its_constraint_accepts() {
    // A typed zero would fail the document gate on the chart's first run.
    let constraint = json!({"type": "integer", "minimum": 8000, "maximum": 9000});
    let values = values_of(&surface(&[key(
        "a.port",
        &json!({"required": true, "text_form": "integer", "constraint": constraint}),
    )]));
    let value = &values["a"]["port"];
    assert_eq!(
        assert_value(&constraint, value, "a.port").expect("a supported constraint"),
        None,
        "{value}"
    );
}

#[test]
fn the_tag_is_quoted_so_a_numeric_one_stays_a_string() {
    let text = render_values(
        "app",
        &Surface::Plain,
        "library/redis",
        "8.0",
        "",
        "config.toml",
    )
    .expect("the values render");
    let values = read_yaml(&text, Path::new("values.yaml")).expect("values are YAML");
    assert_eq!(values["image"]["tag"], json!("8.0"));
}

#[test]
fn the_chassis_is_appended_verbatim() {
    let text = values_text(&Surface::Plain, CHASSIS);
    assert!(text.ends_with(CHASSIS), "{text}");
    let values = read_yaml(&text, Path::new("values.yaml")).expect("values are YAML");
    for name in [
        "podSecurityContextPreset",
        "serviceAccount",
        "networkPolicy",
        "resources",
    ] {
        assert!(values.get(name).is_some(), "{name} is missing");
    }
}

// --------------------------------------------------------------------------------------
// The `@schema` blocks, as helm-schema reads them
// --------------------------------------------------------------------------------------

fn hosts(overrides: &Json) -> Json {
    let mut held = json!({
        "text_form": "structured",
        "default_value": ["one"],
        "constraint": {"type": "array", "items": {"type": "string"}},
    });
    for (name, value) in overrides.as_object().expect("an object") {
        held[name] = value.clone();
    }
    key("a.hosts", &held)
}

#[test]
fn a_described_element_is_written_without_enrolling_it() {
    // The marker inside the block is what makes it generated, so the interlock holding it to
    // the contract is there from the first shapes run without a second line asserting it.
    let text = values_text(&surface(&[hosts(&json!({}))]), "");
    assert!(
        text.contains("# # @config structured a.hosts optional"),
        "{text}"
    );
    assert!(!text.contains("@config-shape"), "{text}");
}

#[test]
fn an_undescribed_container_is_written_without_a_marker_either() {
    let text = values_text(
        &surface(&[key(
            "a.peers",
            &json!({"text_form": "structured", "constraint": {"type": "object"}}),
        )]),
        "",
    );
    assert!(!text.contains("@config-shape"), "{text}");
}

#[test]
fn a_structured_arrays_default_is_a_value_its_own_block_accepts() {
    let text = values_text(&surface(&[hosts(&json!({}))]), "");
    let (block, default) = block_for(&text, "hosts");
    let types = block["type"].as_array().expect("a type list");
    assert!(
        types.contains(&json!(json_type(&default))),
        "{default} against {block}"
    );
}

#[test]
fn no_generated_block_carries_both_enum_and_type() {
    // helm-schema refuses one carrying both, fatally, and then writes no schema for any chart.
    // The property, over a surface holding one of every shape and the chassis after it.
    let text = values_text(
        &surface(&[
            key("a.text", &json!({})),
            key(
                "a.port",
                &json!({"text_form": "integer", "constraint": {"type": "integer", "minimum": 1}}),
            ),
            key(
                "a.backend",
                &json!({"text_form": "choice",
                            "constraint": {"type": "string", "enum": ["sqlite", "postgres"]}}),
            ),
            hosts(&json!({"default_value": null})),
            key(
                "a.peers",
                &json!({"text_form": "structured", "constraint": {"type": "object"}}),
            ),
            key("a.token", &json!({"secret": true})),
        ]),
        CHASSIS,
    );
    let found = blocks(&text);
    assert!(found.len() > 10, "{found:?}");
    for (name, block, _) in found {
        assert!(
            !(block.get("enum").is_some() && block.get("type").is_some()),
            "{name}: {block}"
        );
    }
}

// --------------------------------------------------------------------------------------
// Descriptions
// --------------------------------------------------------------------------------------

#[test]
fn the_contracted_form_leaves_nothing_undocumented() {
    let structured_array = json!({"text_form": "structured",
                                      "constraint": {"type": "array", "items": {"type": "string"}}});
    let text = values_text(
        &surface(&[
            key("alertmanager.endpoints", &structured_array),
            key("discord.capabilities.view", &structured_array),
            key(
                "storage.backend",
                &json!({"text_form": "choice",
                            "constraint": {"type": "string", "enum": ["sqlite", "postgres"]}}),
            ),
            key(
                "routes",
                &json!({"text_form": "structured", "constraint": {"type": "array"}}),
            ),
            key("discord.token", &json!({"secret": true})),
        ]),
        CHASSIS,
    );
    assert_eq!(undocumented(&text), Vec::<String>::new());
}

#[test]
fn the_plain_form_leaves_nothing_undocumented() {
    assert_eq!(
        undocumented(&values_text(&Surface::Plain, CHASSIS)),
        Vec::<String>::new()
    );
}

#[test]
fn the_rule_restated_here_does_report_a_missing_description() {
    // A gate that reports nothing for every input would pass the two cases above.
    let text = "# @schema\n# type: string\n# @schema\nbare: x\n\ngroup:\n  # -- Fine.\n  leaf: 1\n";
    assert_eq!(undocumented(text), ["bare", "group"]);
}

#[test]
fn a_grouping_block_names_the_prefix_it_holds() {
    let text = values_text(
        &surface(&[key("discord.capabilities.view", &json!({}))]),
        "",
    );
    assert!(
        text.contains("# -- TODO: what the `discord` settings have in common"),
        "{text}"
    );
    assert!(
        text.contains("# -- TODO: what the `discord.capabilities` settings have in common"),
        "{text}"
    );
}

#[test]
fn the_prefix_is_the_contract_spelling_not_the_values_one() {
    let text = values_text(&surface(&[key("rate_limit.per_ip.burst", &json!({}))]), "");
    assert!(text.contains("`rate_limit.per_ip`"), "{text}");
    assert!(!text.contains("`rateLimit.perIp`"), "{text}");
}

#[test]
fn every_placeholder_is_reported() {
    let held = surface(&[
        key("a.b.c", &json!({})),
        key("d.token", &json!({"secret": true})),
        key("e", &json!({})),
    ]);
    assert_eq!(
        undescribed(held.plan()).expect("the tree builds"),
        ["a", "a.b", "d"]
    );
}

#[test]
fn a_surface_with_no_nesting_reports_none() {
    let held = surface(&[key("a", &json!({}))]);
    assert_eq!(
        undescribed(held.plan()).expect("the tree builds"),
        Vec::<String>::new()
    );
}

#[test]
fn the_chassis_blocks_carry_a_sentence_rather_than_a_placeholder() {
    // `image` and `configMount` are the same in every chart, so neither is a TODO.
    let found = described(&values_text(&surface(&[key("a.b", &json!({}))]), ""));
    for name in ["image", "configMount"] {
        let line = found[name].as_str().expect("described");
        assert!(!line.contains("TODO"), "{name}: {line}");
    }
}

// --------------------------------------------------------------------------------------
// `_helpers.tpl`
// --------------------------------------------------------------------------------------

#[test]
fn an_optional_key_is_wrapped_so_an_unset_value_is_absent() {
    // To the loader an empty value is a supplied value, not an unset one.
    assert!(helpers(&[key("a.b", &json!({}))]).contains("{{- with .Values.a.b }}"));
}

#[test]
fn a_required_key_is_written_unconditionally() {
    let text = helpers(&[key("a.b", &json!({"required": true}))]);
    assert!(text.contains("b: {{ .Values.a.b | quote }}"), "{text}");
    assert!(!text.contains("with .Values.a.b"), "{text}");
}

#[test]
fn a_numeric_key_is_not_quoted() {
    let text = helpers(&[key(
        "a.port",
        &json!({"required": true, "text_form": "integer", "constraint": {"type": "integer"}}),
    )]);
    assert!(text.contains("port: {{ .Values.a.port }}"), "{text}");
}

#[test]
fn an_optional_number_or_boolean_is_guarded_against_null_not_falsy() {
    // `with` would drop a deliberate `0` or `false`, each a real setting and not "unset", and the
    // rule holds whether the contract spells the type as a list or as a bare string.
    for (leaf, kind) in [
        ("count", json!(["integer", "null"])),
        ("enabled", json!(["boolean", "null"])),
        ("bare", json!("integer")),
    ] {
        let text = helpers(&[key(
            &format!("a.{leaf}"),
            &json!({"constraint": {"type": kind}}),
        )]);
        assert!(
            text.contains(&format!(
                "{{{{- if not (kindIs \"invalid\" .Values.a.{leaf}) }}}}"
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!("{leaf}: {{{{ .Values.a.{leaf} }}}}")),
            "{text}"
        );
        assert!(!text.contains(&format!("with .Values.a.{leaf}")), "{text}");
    }
}

#[test]
fn every_credential_reaches_the_secret_under_its_published_file_name() {
    let text = helpers(&[key("a.token", &json!({"secret": true}))]);
    assert!(
        text.contains("a__token: {{ .Values.a.token | quote }}"),
        "{text}"
    );
}

#[test]
fn a_required_credential_is_guarded_against_the_projected_key_list() {
    // Against the list rather than the value, so an `existingSecret` counts: the chart cannot
    // see inside one, and a credential it cannot see is not a credential that is missing.
    let text = helpers(&[key("a.token", &json!({"secret": true, "required": true}))]);
    assert!(
        text.contains("$projected := include \"app.secretKeys\""),
        "{text}"
    );
    assert!(text.contains("has \"a__token\" $projected"), "{text}");
}

#[test]
fn the_template_braces_are_balanced() {
    let text = helpers(&[
        key("a.b", &json!({})),
        key("c.token", &json!({"secret": true, "required": true})),
    ]);
    assert_eq!(
        text.matches("{{").count(),
        text.matches("}}").count(),
        "{text}"
    );
}

// --------------------------------------------------------------------------------------
// The declaration and the enrolment, through their readers
// --------------------------------------------------------------------------------------

#[test]
fn the_declaration_is_one_the_loader_accepts() {
    let held = surface(&[
        key("a.b", &json!({})),
        key("c.token", &json!({"secret": true})),
    ]);
    let text = render_declaration("app", "app", "config.toml", held.plan())
        .expect("the declaration renders");
    let declaration =
        parse_declaration(&text, Path::new("charts/app")).expect("the loader accepts it");
    assert!(declaration.bindings);
    assert!(declaration.tests);
    let names: Vec<&str> = declaration
        .documents
        .iter()
        .map(|document| document.name.as_str())
        .collect();
    assert_eq!(names, ["app"]);
    let written_off: Vec<&str> = declaration
        .unbound
        .iter()
        .flat_map(|entry| entry.keys.iter().map(String::as_str))
        .collect();
    assert_eq!(written_off, ["c.token"]);
}

#[test]
fn a_scaffolded_declaration_writes_only_what_differs_from_the_defaults() {
    let held = surface(&[key("a.b", &json!({}))]);
    let text = render_declaration("app", "app", "config.toml", held.plan())
        .expect("the declaration renders");
    let declaration =
        parse_declaration(&text, Path::new("charts/app")).expect("the loader accepts it");
    let document = &declaration.documents[0];
    assert!(document.consumers.inferred());
    assert_eq!(document.images[0].contract, "contracts/app.json");
    assert!(!text.contains("source:"), "{text}");
    assert_eq!(
        crate::helm::declaration::schema_violations(&text),
        Vec::<String>::new(),
        "{text}"
    );
    let lines = text.lines().filter(|line| !line.trim().is_empty()).count();
    assert!(lines < 15, "{lines} lines:\n{text}");

    let keyed = render_declaration("app", "app", "settings.toml", held.plan())
        .expect("the declaration renders");
    let declaration =
        parse_declaration(&keyed, Path::new("charts/app")).expect("the loader accepts it");
    assert_eq!(declaration.documents[0].source.key, "settings.toml");

    // The scaffold renders TOML whatever the key is called.
    let misleading = render_declaration("app", "app", "config.yaml", held.plan())
        .expect("the declaration renders");
    let declaration =
        parse_declaration(&misleading, Path::new("charts/app")).expect("the loader accepts it");
    assert_eq!(
        declaration.documents[0].source.format,
        crate::gate::DocumentFormat::Toml
    );
}

/// The scaffolded declaration's first document, as YAML: what its `tests` block says.
fn enrolment(keys: &[Json]) -> Json {
    let held = surface(keys);
    let text = render_declaration("app", "app", "config.toml", held.plan())
        .expect("the declaration renders");
    parse_declaration(&text, Path::new("charts/app")).expect("the loader accepts it");
    assert_eq!(
        crate::helm::declaration::schema_violations(&text),
        Vec::<String>::new(),
        "{text}"
    );
    let parsed = read_yaml(&text, Path::new("config-contract.yaml")).expect("it is YAML");
    parsed["documents"][0]
        .get("tests")
        .cloned()
        .unwrap_or_else(|| json!({}))
}

#[test]
fn a_chart_with_no_required_credential_needs_no_prerequisite() {
    let parsed = enrolment(&[
        key("a.b", &json!({})),
        key("c.token", &json!({"secret": true})),
    ]);
    assert_eq!(parsed.get("prerequisites"), None);
}

#[test]
fn a_required_credential_becomes_a_prerequisite_with_a_reason() {
    let parsed = enrolment(&[key("a.token", &json!({"secret": true, "required": true}))]);
    let prerequisites = &parsed["prerequisites"];
    let names: Vec<&String> = prerequisites["values"]
        .as_object()
        .expect("values")
        .keys()
        .collect();
    assert_eq!(names, ["a.token"]);
    let reason = prerequisites["reason"].as_str().expect("a reason");
    assert_ne!(reason.trim(), "", "{prerequisites}");
}

#[test]
fn no_prerequisite_names_a_path_under_the_probe_root() {
    // A prerequisite is dropped from no case, so one under `config` would supply a probe.
    let parsed = enrolment(&[key("a.token", &json!({"secret": true, "required": true}))]);
    for path in parsed["prerequisites"]["values"]
        .as_object()
        .expect("values")
        .keys()
    {
        assert!(
            path != CONFIG_ROOT && !path.starts_with(&format!("{CONFIG_ROOT}.")),
            "{path}"
        );
    }
}

#[test]
fn an_invented_credential_can_never_authenticate() {
    let parsed = enrolment(&[key("a.token", &json!({"secret": true, "required": true}))]);
    let value = parsed["prerequisites"]["values"]["a.token"]
        .as_str()
        .expect("a string");
    assert!(value.ends_with(".invalid"), "{value}");
}

#[test]
fn a_patterned_credential_is_refused_rather_than_guessed_at() {
    let held = surface(&[key(
        "a.token",
        &json!({"secret": true, "required": true,
                    "constraint": {"type": "string", "pattern": "^tok_"}}),
    )]);
    assert!(probe_credential(&held.plan().credentials[0]).is_err());
    assert!(render_declaration("app", "app", "config.toml", held.plan()).is_err());
    assert!(render_ci_values("app", &held).is_err());
}

#[test]
fn the_fixture_carries_each_required_credential_and_nothing_else() {
    let held = surface(&[
        key("a.b", &json!({"required": true})),
        key("a.token", &json!({"secret": true, "required": true})),
        key("c.token", &json!({"secret": true})),
    ]);
    let text = render_ci_values("app", &held).expect("the fixture renders");
    let parsed = read_yaml(&text, Path::new("ci/test-values.yaml")).expect("YAML");
    assert_eq!(
        parsed,
        json!({"a": {"token": "scaffolded-placeholder.a-token.invalid"}})
    );
}

// --------------------------------------------------------------------------------------
// The plain form
// --------------------------------------------------------------------------------------

#[test]
fn no_configuration_surface_is_asserted() {
    let values = values_of(&Surface::Plain);
    for name in ["config", "configExtraToml", "configMount", "existingSecret"] {
        assert!(
            values.get(name).is_none(),
            "{name} describes a loader nothing knows this image has"
        );
    }
}

#[test]
fn the_image_block_is_still_written() {
    assert_eq!(
        values_of(&Surface::Plain)["image"]["repository"],
        json!("org/app")
    );
}

#[test]
fn the_helpers_file_defines_nothing() {
    let text = render_helpers("app", &Surface::Plain).expect("the helpers render");
    assert!(!text.contains("{{- define"), "{text}");
    assert!(text.contains("{{/*"), "{text}");
}

#[test]
fn the_fixture_installs_the_defaults() {
    let text = render_ci_values("app", &Surface::Plain).expect("the fixture renders");
    assert_eq!(
        read_yaml(&text, Path::new("ci/test-values.yaml")).expect("YAML"),
        json!({})
    );
}

#[test]
fn the_form_selects_the_template_subtree() {
    assert_eq!(Surface::Plain.form(), "plain");
    assert_eq!(surface(&[key("a.b", &json!({}))]).form(), "contract");
}

#[test]
fn a_contract_with_no_nesting_separator_is_refused() {
    let mut held = union_of(&[key("a.b", &json!({}))]);
    held.dialect.remove("nesting_separator");
    assert!(Surface::from_contract(&held).is_err());
}
