//! A chart's declaration with every default written out: what `config show --chart` prints.
//!
//! A declaration may leave most of itself to defaults (see `spec/helm/v1/DEFAULTS.md`), which keeps
//! the file short and makes the question "what is this chart actually held to?" one a reader cannot
//! answer from the file alone. This answers it, in the file's own spelling, so the output is itself
//! a declaration: read back, it means exactly what the original meant.
//!
//! Two things cannot be written out and are left out instead, which is what they mean when read
//! back. Inferred consumers are found again in each rendered values file, so there is no one list
//! to print. And the chart-level `tests` switch is printed together with each document's `tests`
//! block, wherever the enrolment was read from: a chart still enrolled through the older
//! `contract-tests.yaml` gets the declaration that replaces it.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::Value as Json;
use serde_norway::{Mapping, Value as Yaml};

use super::declaration::{Consumers, Declaration, Document};
use super::suites::{Enrolment, Enrolments};

/// The declaration, with each default in place, ready to print.
#[derive(Debug, Clone)]
pub struct Effective {
    /// The declaration in the file's spelling, keys in the order the reference lists them.
    pub declaration: Yaml,
    /// Each document's defaulted fields, by document name, as dotted paths under the document.
    pub defaulted: BTreeMap<String, Vec<&'static str>>,
    /// Where the enrolment was read from, when the chart is enrolled.
    pub enrolment: Option<String>,
}

impl Effective {
    /// The same, as one JSON object: `declaration`, `defaulted` and `enrolment`.
    ///
    /// # Panics
    /// Never: every value here came from JSON or a string, and both convert.
    #[must_use]
    pub fn to_json(&self) -> Json {
        serde_json::json!({
            "declaration": serde_json::to_value(&self.declaration)
                .expect("a declaration read from YAML converts to JSON"),
            "defaulted": self.defaulted,
            "enrolment": self.enrolment,
        })
    }

    /// The declaration as YAML, followed by a comment naming every field a default supplied.
    ///
    /// # Panics
    /// Never: the value is built here from strings, booleans and JSON, all of which serialise.
    #[must_use]
    pub fn to_yaml(&self) -> String {
        let mut text = serde_norway::to_string(&self.declaration)
            .expect("a declaration built from plain values serialises");
        if let Some(path) = &self.enrolment {
            writeln!(text, "# Round-trip enrolment read from {path}.")
                .expect("a String takes a write");
        }
        let defaulted: Vec<String> = self
            .defaulted
            .iter()
            .flat_map(|(name, fields)| {
                fields.iter().map(move |field| match *field {
                    "consumers" => format!(
                        "documents[{name}].consumers (inferred: the workloads that mount the \
                         source, in each rendered values file)"
                    ),
                    other => format!("documents[{name}].{other}"),
                })
            })
            .collect();
        if defaulted.is_empty() {
            text.push_str("# No field is taken from a default.\n");
        } else {
            text.push_str("# Taken from defaults (spec/helm/v1/DEFAULTS.md):\n");
            for line in defaulted {
                writeln!(text, "#   {line}").expect("a String takes a write");
            }
        }
        text
    }
}

/// The declaration with every default written out, and the enrolment read for it.
///
/// `enrolments` is what [`super::suites::load_enrolment`] returned for the chart, so a chart enrolled
/// through either file is shown the same way.
#[must_use]
pub fn effective(declaration: &Declaration, enrolments: Option<&Enrolments>) -> Effective {
    let mut top = Mapping::new();
    if declaration.bindings {
        top.insert(text("bindings"), Yaml::Bool(true));
    }
    if enrolments.is_some() {
        top.insert(text("tests"), Yaml::Bool(true));
    }
    if let Some(reason) = &declaration.reason {
        top.insert(text("reason"), text(reason));
    }
    if !declaration.unconfigured.is_empty() {
        top.insert(text("unconfigured"), strings(&declaration.unconfigured));
    }
    if !declaration.unbound.is_empty() {
        let entries = declaration.unbound.iter().map(|group| {
            let mut entry = Mapping::new();
            entry.insert(text("keys"), strings(&group.keys));
            entry.insert(text("reason"), text(&group.reason));
            if let Some(documents) = &group.documents {
                entry.insert(text("documents"), strings(documents));
            }
            Yaml::Mapping(entry)
        });
        top.insert(text("unbound"), Yaml::Sequence(entries.collect()));
    }
    if !declaration.credentials.is_empty() {
        let entries = declaration.credentials.values().map(|note| {
            let mut entry = Mapping::new();
            entry.insert(text("key"), text(&note.key));
            if let Some(value) = &note.value {
                entry.insert(text("value"), text(value));
            }
            if let Some(note) = &note.note {
                entry.insert(text("note"), text(note));
            }
            Yaml::Mapping(entry)
        });
        top.insert(text("credentials"), Yaml::Sequence(entries.collect()));
    }

    let documents = declaration.documents.iter().map(|document| {
        let enrolment = enrolments.map(|enrolled| {
            enrolled
                .documents
                .get(&document.name)
                .cloned()
                .unwrap_or_default()
        });
        document_value(document, enrolment.as_ref())
    });
    top.insert(text("documents"), Yaml::Sequence(documents.collect()));

    Effective {
        declaration: Yaml::Mapping(top),
        defaulted: declaration
            .documents
            .iter()
            .filter(|document| !document.defaulted.is_empty())
            .map(|document| (document.name.clone(), document.defaulted.clone()))
            .collect(),
        enrolment: enrolments.map(|enrolled| enrolled.path.display().to_string()),
    }
}

/// One document, every field written out.
fn document_value(document: &Document, enrolment: Option<&Enrolment>) -> Yaml {
    let mut entry = Mapping::new();
    entry.insert(text("name"), text(&document.name));

    let mut source = Mapping::new();
    source.insert(text("kind"), text(&document.source.kind));
    source.insert(text("selector"), pairs(&document.source.selector));
    source.insert(text("key"), text(&document.source.key));
    source.insert(text("format"), text(document.source.format.label()));
    entry.insert(text("source"), Yaml::Mapping(source));

    let images = document.images.iter().map(|image| {
        let mut held = Mapping::new();
        held.insert(text("values"), text(&image.values));
        held.insert(text("contract"), text(&image.contract));
        Yaml::Mapping(held)
    });
    entry.insert(text("images"), Yaml::Sequence(images.collect()));

    if let Consumers::Declared(listed) = &document.consumers {
        let consumers = listed.iter().map(|consumer| {
            let mut workload = Mapping::new();
            workload.insert(text("kind"), text(&consumer.kind));
            workload.insert(text("selector"), pairs(&consumer.selector));
            let mut held = Mapping::new();
            held.insert(text("workload"), Yaml::Mapping(workload));
            held.insert(text("containers"), strings(&consumer.containers));
            Yaml::Mapping(held)
        });
        entry.insert(text("consumers"), Yaml::Sequence(consumers.collect()));
    }

    if !document.exempt.is_empty() {
        let exempt = document.exempt.iter().map(|exemption| {
            let mut held = Mapping::new();
            held.insert(text("values"), text(&exemption.values));
            held.insert(text("gates"), strings(&exemption.gates));
            held.insert(text("reason"), text(&exemption.reason));
            Yaml::Mapping(held)
        });
        entry.insert(text("exempt"), Yaml::Sequence(exempt.collect()));
    }

    if !document.restart.is_empty() {
        let restart = document.restart.iter().map(|group| {
            let mut held = Mapping::new();
            held.insert(text("keys"), strings(&group.keys));
            held.insert(text("reason"), text(&group.reason));
            Yaml::Mapping(held)
        });
        entry.insert(text("restart"), Yaml::Sequence(restart.collect()));
    }

    if let Some(block) = enrolment.and_then(tests_value) {
        entry.insert(text("tests"), block);
    }
    Yaml::Mapping(entry)
}

/// A document's `tests` block; [`None`] when every field is its default.
fn tests_value(enrolment: &Enrolment) -> Option<Yaml> {
    let defaults = Enrolment::default();
    let mut block = Mapping::new();
    if enrolment.probe != defaults.probe {
        block.insert(text("probe"), text(&enrolment.probe));
    }
    if !enrolment.baseline.values.is_empty() {
        block.insert(text("baseline"), json_pairs(&enrolment.baseline.values));
    }
    if let Some(reason) = &enrolment.baseline.reason {
        block.insert(text("reason"), text(reason));
    }
    if !enrolment.prerequisites.values.is_empty() {
        let mut held = Mapping::new();
        held.insert(text("values"), json_pairs(&enrolment.prerequisites.values));
        if let Some(reason) = &enrolment.prerequisites.reason {
            held.insert(text("reason"), text(reason));
        }
        block.insert(text("prerequisites"), Yaml::Mapping(held));
    }
    if !enrolment.unrouted.is_empty() {
        // Held one key at a time; written back grouped by reason, in first-seen order, which is
        // how a contributor writes them.
        let mut grouped: Vec<(&str, Vec<&str>)> = Vec::new();
        for (key, reason) in &enrolment.unrouted {
            match grouped
                .iter_mut()
                .find(|(held, _)| *held == reason.as_str())
            {
                Some((_, keys)) => keys.push(key),
                None => grouped.push((reason, vec![key])),
            }
        }
        let entries = grouped.into_iter().map(|(reason, keys)| {
            let mut held = Mapping::new();
            held.insert(
                text("keys"),
                Yaml::Sequence(keys.into_iter().map(text).collect()),
            );
            held.insert(text("reason"), text(reason));
            Yaml::Mapping(held)
        });
        block.insert(text("unrouted"), Yaml::Sequence(entries.collect()));
    }
    (!block.is_empty()).then_some(Yaml::Mapping(block))
}

fn text(value: &str) -> Yaml {
    Yaml::String(value.to_owned())
}

fn strings(values: &[String]) -> Yaml {
    Yaml::Sequence(values.iter().map(|value| text(value)).collect())
}

fn pairs(values: &[(String, String)]) -> Yaml {
    Yaml::Mapping(
        values
            .iter()
            .map(|(key, value)| (text(key), text(value)))
            .collect(),
    )
}

/// `set` pairs whose leaves may be any JSON value.
///
/// # Panics
/// Never: a JSON value always converts to YAML.
fn json_pairs(values: &[(String, Json)]) -> Yaml {
    Yaml::Mapping(
        values
            .iter()
            .map(|(key, value)| {
                let leaf = serde_norway::to_value(value).expect("JSON converts to YAML");
                (text(key), leaf)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::helm::declaration::parse_declaration;

    fn parsed(body: &str) -> Declaration {
        parse_declaration(body, Path::new("charts/app")).expect("the declaration parses")
    }

    fn enrolled(declaration: &Declaration) -> Option<Enrolments> {
        declaration.tests.then(|| Enrolments {
            path: PathBuf::from("charts/app/config-contract.yaml"),
            documents: declaration
                .documents
                .iter()
                .filter_map(|document| Some((document.name.clone(), document.tests.clone()?)))
                .collect(),
        })
    }

    #[test]
    fn every_default_is_written_out_and_named() {
        let declaration = parsed("documents:\n  - name: app\n");
        let shown = effective(&declaration, None);
        let yaml = shown.to_yaml();

        assert!(yaml.contains("app.kubernetes.io/instance: app"), "{yaml}");
        assert!(yaml.contains("key: config.toml"), "{yaml}");
        assert!(yaml.contains("format: toml"), "{yaml}");
        assert!(yaml.contains("contract: contracts/app.json"), "{yaml}");
        assert!(
            !yaml.contains("consumers:"),
            "inferred consumers have no list: {yaml}"
        );
        assert!(
            yaml.contains("#   documents[app].source.selector"),
            "{yaml}"
        );
        assert!(
            yaml.contains("documents[app].consumers (inferred"),
            "{yaml}"
        );
    }

    #[test]
    fn the_printed_declaration_reads_back_to_itself() {
        let body = "\
bindings: true
tests: true
unbound:
  - keys: [a.b]
    reason: not surfaced
documents:
  - name: app
    source:
      key: settings.yaml
    consumers:
      - workload: { kind: Deployment, selector: { app: x } }
        containers: [app]
    tests:
      prerequisites:
        values: { auth.existingSecret: creds }
        reason: the chart refuses to render without a secret
      unrouted:
        - keys: [a.b, a.c]
          reason: guarded
";
        let declaration = parsed(body);
        let once = effective(&declaration, enrolled(&declaration).as_ref());

        let printed = serde_norway::to_string(&once.declaration).expect("serialises");
        assert_eq!(
            crate::helm::declaration::schema_violations(&printed),
            Vec::<String>::new(),
            "{printed}"
        );
        let again = parsed(&printed);
        let twice = effective(&again, enrolled(&again).as_ref());

        assert_eq!(once.declaration, twice.declaration, "{printed}");
        assert!(
            twice.defaulted.is_empty(),
            "a printed declaration leaves nothing to a default it can write: {:?}",
            twice.defaulted
        );
    }

    #[test]
    fn an_enrolment_from_the_older_file_is_shown_as_the_declarations_tests() {
        let declaration = parsed("documents:\n  - name: app\n");
        let legacy = Enrolments {
            path: PathBuf::from("charts/app/contract-tests.yaml"),
            documents: BTreeMap::new(),
        };
        let shown = effective(&declaration, Some(&legacy));

        let yaml = shown.to_yaml();
        assert!(yaml.starts_with("tests: true\n"), "{yaml}");
        assert!(
            yaml.contains("read from charts/app/contract-tests.yaml"),
            "{yaml}"
        );
        assert_eq!(
            shown.to_json()["enrolment"],
            "charts/app/contract-tests.yaml"
        );
    }
}
