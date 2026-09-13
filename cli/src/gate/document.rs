//! Gate 1 — the rendered document, against the union of every contract that reads it.
//!
//! Select the object by kind and label selector, take `data[key]`, parse it, and validate the
//! result against the merged `json_schema` with `additionalProperties: false`. That catches an
//! unknown key, a wrong type, a missing required key, a value outside an enum, and a table where a
//! scalar belongs — the whole class of defect that renders cleanly, passes every schema a chart has
//! for its own values, passes a Kubernetes validator, and then boots on a compiled default nobody
//! chose.
//!
//! # The engine is linked rather than shelled out to
//!
//! The implementation this was ported from delegated to a pinned Go binary, to keep a chart
//! repository's scripts on its standard library. A build that already links a JSON Schema engine for
//! `validate` has no such constraint, and one fewer pinned binary is one fewer thing a recipe can
//! fail to find. Its default features are off, so the engine cannot fetch a remote `$ref` even if
//! one reached it — and [`local_refs_only`](crate::union::local_refs_only) refuses one before it
//! gets that far, because an offline gate that resolves a remote reference has silently become a
//! networked one.

use serde_json::Value as Json;

use crate::error::Error;
use crate::k8s::{names_of, select};
use crate::report::{Finding, error};
use crate::union::{Union, suggest};

use super::{Relaxed, json_text, quoted};

/// How a rendered document is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocumentFormat {
    /// A TOML file, which is what a loader in this family reads.
    #[default]
    Toml,
    /// A JSON file.
    Json,
    /// A YAML file.
    Yaml,
}

impl DocumentFormat {
    /// The spelling a declaration uses, and the one a message names.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Yaml => "yaml",
        }
    }

    /// The format one spelling names, or [`None`] for a spelling outside the three.
    pub fn parse(spelling: &str) -> Option<Self> {
        match spelling {
            "toml" => Some(Self::Toml),
            "json" => Some(Self::Json),
            "yaml" => Some(Self::Yaml),
            _ => None,
        }
    }

    /// Read one rendered document.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the text is not this format, carrying the parser's own message —
    /// which names the line, and is the half of the answer a reader actually needs.
    pub fn read(self, text: &str) -> Result<Json, Error> {
        let failed = |failure: &dyn std::fmt::Display| Error::Invalid(failure.to_string());
        match self {
            Self::Toml => toml::from_str::<Json>(text).map_err(|e| failed(&e)),
            Self::Json => serde_json::from_str::<Json>(text).map_err(|e| failed(&e)),
            Self::Yaml => serde_norway::from_str::<Json>(text).map_err(|e| failed(&e)),
        }
    }
}

/// Which rendered object holds a configuration document, and where inside it.
///
/// Not a chart concept: it is "the object of this kind carrying these labels, under this key, in
/// this format", which is answerable of any Kubernetes tree. What decides the values of these
/// fields is a chart's own declaration, one feature up.
#[derive(Debug, Clone, Default)]
pub struct DocumentSource {
    /// The object's kind, e.g. `ConfigMap`.
    pub kind: String,
    /// The labels that name it, rather than the rendered name, which a name override moves.
    pub selector: Vec<(String, String)>,
    /// The key inside `data` holding the document.
    pub key: String,
    /// How the document is written.
    pub format: DocumentFormat,
}

/// Validate one rendered configuration document against the union of its contracts.
///
/// [`None`] means the selector matched nothing at all, which is distinct from "no findings": a chart
/// can switch a whole component off in one values file and on in another, and the two need different
/// answers. The caller decides which, because it can see whether the consumers are absent too and
/// whether any other values file rendered this document.
///
/// # Errors
/// [`Error::Invalid`] when the merged schema does not compile, which is a defect in the contracts
/// rather than in the tree.
pub fn check_document(
    manifests: &[Json],
    source: &DocumentSource,
    union: &Union,
    relaxed: Relaxed,
) -> Result<Option<Vec<Finding>>, Error> {
    if relaxed.document {
        return Ok(Some(Vec::new()));
    }

    let matched = select(manifests, &source.kind, &source.selector);
    if matched.is_empty() {
        return Ok(None);
    }
    if matched.len() != 1 {
        return Ok(Some(vec![error(format!(
            "the selector {} matches {} {}s ({}), and a document must name exactly one",
            selector_text(&source.selector),
            matched.len(),
            source.kind,
            names_of(&matched)
        ))]));
    }

    let data = matched[0].get("data");
    let Some(held) = data.and_then(|data| data.get(&source.key)) else {
        let name = matched[0]
            .get("metadata")
            .and_then(|metadata| metadata.get("name"))
            .and_then(Json::as_str)
            .unwrap_or("?");
        let candidates: Vec<&str> = data
            .and_then(Json::as_object)
            .map(|data| data.keys().map(String::as_str).collect())
            .unwrap_or_default();
        return Ok(Some(vec![error(format!(
            "{} {} has no key {}{}",
            source.kind,
            quoted(name),
            quoted(&source.key),
            suggest(&source.key, candidates)
        ))]));
    };

    let text = held.as_str().unwrap_or_default();
    let instance = match source.format.read(text) {
        Ok(instance) => instance,
        Err(failure) => {
            return Ok(Some(vec![error(format!(
                "{}: is not valid {}: {failure}",
                source.key,
                source.format.label()
            ))]));
        }
    };

    let mut schema = union.json_schema.clone();
    if relaxed.closed {
        open_schema(&mut schema);
    }

    let validator = jsonschema::draft7::options()
        .build(&schema)
        .map_err(|failure| {
            Error::Invalid(format!(
                "the merged schema of {} does not compile, which is a defect in the contracts \
                 rather than in the tree: {failure}",
                union.sources.join(" and ")
            ))
        })?;

    let mut findings = Vec::new();
    for failure in validator.iter_errors(&instance) {
        let path = dotted(&failure.instance_path().to_string());
        match failure.kind() {
            // An unknown key is the rename case, and the union already holds every key path — so
            // the answer to "what was it called instead" costs an edit-distance pass and turns the
            // most common failure here from a puzzle into a one-line answer.
            jsonschema::error::ValidationErrorKind::AdditionalProperties { unexpected } => {
                for name in unexpected {
                    let full = if path.is_empty() {
                        name.clone()
                    } else {
                        format!("{path}.{name}")
                    };
                    findings.push(error(format!(
                        "{}: {full}: no such key{}",
                        source.key,
                        suggest(&full, union.keys.names())
                    )));
                }
            }
            _ => findings.push(error(format!(
                "{}: {}: {failure}",
                source.key,
                if path.is_empty() { "(root)" } else { &path }
            ))),
        }
    }
    Ok(Some(findings))
}

/// Drop every `additionalProperties: false` the union added, for a `closed` exemption.
fn open_schema(schema: &mut Json) {
    match schema {
        Json::Object(fields) => {
            fields.retain(|name, value| {
                !(name == "additionalProperties" && value == &Json::Bool(false))
            });
            for value in fields.values_mut() {
                open_schema(value);
            }
        }
        Json::Array(items) => {
            for value in items {
                open_schema(value);
            }
        }
        _ => {}
    }
}

/// A selector as a message shows it.
///
/// Built by hand rather than through a map, so the labels appear in the order the declaration wrote
/// them: a reader comparing the message against the file they typed should not have to reorder it.
fn selector_text(selector: &[(String, String)]) -> String {
    let fields: Vec<String> = selector
        .iter()
        .map(|(name, value)| format!("{}: {}", json_text(name), json_text(value)))
        .collect();
    format!("{{{}}}", fields.join(", "))
}

/// A JSON pointer as a dotted path a reader can find in the file.
fn dotted(location: &str) -> String {
    location
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use serde_json::{Value as Json, json};

    use super::{DocumentFormat, DocumentSource, Relaxed, check_document};
    use crate::union::{Union, union_contracts};

    fn union(json_schema: &Json, keys: &Json) -> Union {
        let contract = json!({
            "terrace_contract": 1,
            "producer": {"name": "x", "version": "1", "loader": "figment"},
            "app": {"name": "x"},
            "schema": {
                "schema_version": 2,
                "dialect": {"prefix": "P_", "nesting_separator": "__", "indirection_suffix": "_FILE"},
                "loader": [], "keys": keys.clone(),
            },
            "json_schema": json_schema.clone(),
            "external": {"env": [], "ignore": [], "unknown": "reject"},
        });
        union_contracts(&[("a".to_owned(), contract)]).expect("one contract merges")
    }

    fn config_map(body: &str) -> Vec<Json> {
        vec![json!({
            "kind": "ConfigMap",
            "metadata": {"name": "app-config", "labels": {"c": "api"}},
            "data": {"config.toml": body},
        })]
    }

    fn source() -> DocumentSource {
        DocumentSource {
            kind: "ConfigMap".to_owned(),
            selector: vec![("c".to_owned(), "api".to_owned())],
            key: "config.toml".to_owned(),
            format: DocumentFormat::Toml,
        }
    }

    #[test]
    fn a_selector_matching_nothing_is_absent_rather_than_wrong() {
        let found = check_document(
            &[],
            &source(),
            &union(&json!({}), &json!([])),
            Relaxed::default(),
        )
        .expect("the schema compiles");
        assert!(found.is_none());
    }

    #[test]
    fn a_key_the_contract_does_not_declare_is_named_with_what_it_might_have_been() {
        let merged = union(
            &json!({"properties": {"ttl_secs": {"type": "integer"}}}),
            &json!([{"path": "ttl_secs", "text_form": "integer"}]),
        );
        let findings = check_document(
            &config_map("ttl_sec = 30\n"),
            &source(),
            &merged,
            Relaxed::default(),
        )
        .expect("the schema compiles")
        .expect("the document was rendered");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(
            findings[0].message,
            "config.toml: ttl_sec: no such key (did you mean ttl_secs?)"
        );
    }

    #[test]
    fn a_closed_exemption_tolerates_a_key_and_still_checks_the_ones_declared() {
        let merged = union(
            &json!({"properties": {"ttl_secs": {"type": "integer"}}}),
            &json!([{"path": "ttl_secs", "text_form": "integer"}]),
        );
        let relaxed = Relaxed {
            closed: true,
            ..Relaxed::default()
        };
        let findings = check_document(
            &config_map("extra = 1\nttl_secs = \"x\"\n"),
            &source(),
            &merged,
            relaxed,
        )
        .expect("the schema compiles")
        .expect("the document was rendered");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].message.starts_with("config.toml: ttl_secs: "),
            "{findings:?}"
        );
    }

    #[test]
    fn a_document_that_is_not_the_format_it_claims_says_so_once() {
        let findings = check_document(
            &config_map("this is not toml"),
            &source(),
            &union(&json!({}), &json!([])),
            Relaxed::default(),
        )
        .expect("the schema compiles")
        .expect("the document was rendered");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0]
                .message
                .starts_with("config.toml: is not valid toml:"),
            "{findings:?}"
        );
    }

    #[test]
    fn a_selector_matching_several_objects_is_a_finding_rather_than_a_choice() {
        let mut manifests = config_map("");
        manifests.push(manifests[0].clone());
        let findings = check_document(
            &manifests,
            &source(),
            &union(&json!({}), &json!([])),
            Relaxed::default(),
        )
        .expect("the schema compiles")
        .expect("the document was rendered");
        assert!(
            findings[0].message.contains("matches 2 ConfigMaps"),
            "{findings:?}"
        );
    }

    #[test]
    fn a_missing_key_names_the_object_and_what_it_might_have_been() {
        let manifests = vec![json!({
            "kind": "ConfigMap",
            "metadata": {"name": "app-config", "labels": {"c": "api"}},
            "data": {"config.tml": ""},
        })];
        let findings = check_document(
            &manifests,
            &source(),
            &union(&json!({}), &json!([])),
            Relaxed::default(),
        )
        .expect("the schema compiles")
        .expect("the document was rendered");
        assert_eq!(
            findings[0].message,
            "ConfigMap 'app-config' has no key 'config.toml' (did you mean config.tml?)"
        );
    }

    #[test]
    fn a_relaxed_document_gate_checks_nothing_and_is_still_present() {
        let relaxed = Relaxed {
            document: true,
            ..Relaxed::default()
        };
        let found = check_document(&[], &source(), &union(&json!({}), &json!([])), relaxed)
            .expect("the schema compiles");
        assert_eq!(found, Some(Vec::new()));
    }
}
