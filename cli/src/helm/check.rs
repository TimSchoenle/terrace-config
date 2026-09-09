//! Walking the charts, and handing each rendered pair to the gates.
//!
//! The loop, the wiring and nothing else. Every rule it composes lives beside it, one concern per
//! module, so each is testable by calling it — and every gate runs before anything decides an exit
//! code, so one broken chart does not hide the state of the rest.
//!
//! # Which contract belongs to which container is derived, not declared
//!
//! Every declared image resolves to a digest, every rendered container names one, and the two are
//! matched. A hand-written mapping would duplicate a fact the manifest already states, and could
//! drift from it.

use std::path::Path;

use serde_json::Value as Json;

use crate::error::Error;
use crate::gate::{Relaxed, ServiceLinks, check_container, check_document};
use crate::k8s::{containers_of, digest_of, load_manifests, pod_spec, select};
use crate::report::{Report, warning};
use crate::union::Union;
use crate::value::reads_for;

use super::declaration::{Binding, Consumer, Declaration, Document, bind, declared, read_yaml};
use super::{DECLARATION, dig};

/// What a run of [`check`] found, and how much of the tree it looked at.
#[derive(Debug)]
pub struct Checked {
    /// Everything the gates had to say.
    pub report: Report,
    /// How many charts declared a contract. Zero is worth saying out loud: it means the whole run
    /// validated nothing, which looks exactly like a clean run from the outside.
    pub charts: usize,
}

/// Check every chart that declares a contract against the manifests a render produced.
///
/// # Errors
/// [`Error::Invalid`] when a declaration or a contract cannot be read at all, and [`Error::Io`] when
/// the trees cannot be walked. A tree this cannot read is a different outcome from a tree it read
/// and found wanting, and a pipeline that treated them the same could not tell a failing gate from a
/// broken one.
pub fn check(charts: &Path, rendered: &Path) -> Result<Checked, Error> {
    let mut report = Report::new();
    let mut count = 0;

    for (chart_dir, declaration) in declared(charts, true)? {
        check_chart(&chart_dir, &declaration, rendered, &mut report)?;
        count += 1;
    }

    Ok(Checked {
        report,
        charts: count,
    })
}

/// Every document one chart declares.
fn check_chart(
    chart_dir: &Path,
    declaration: &Declaration,
    rendered: &Path,
    report: &mut Report,
) -> Result<(), Error> {
    let values = read_file(&chart_dir.join("values.yaml"))?;
    let chart_yaml = read_file(&chart_dir.join("Chart.yaml"))?;
    let app_version = chart_yaml.get("appVersion").and_then(Json::as_str);

    for document in &declaration.documents {
        let at = format!("{}: {}", declaration.chart, document.name);

        let (binding, problems) = bind(chart_dir, document, &values, app_version)?;
        for problem in problems {
            report.fail(&at, problem);
        }
        let Some(binding) = binding else { continue };

        report_unchecked_range(&at, &binding.union, report);

        let pairs = rendered_pairs(rendered, &declaration.chart)?;
        let mut rendered_anywhere = false;
        let mut containers_seen: Vec<String> = Vec::new();
        for pair in &pairs {
            let (present, found) = check_pair(pair, document, &binding, report)?;
            rendered_anywhere = rendered_anywhere || present;
            for name in found {
                if !containers_seen.contains(&name) {
                    containers_seen.push(name);
                }
            }
        }

        // A chart may switch a whole component off in one values file, and a document that is not
        // rendered cannot be validated. Skipping it silently would also skip a component whose
        // label was renamed, so the two are told apart by scope: absent from *this* pair is normal,
        // absent from *every* pair means the declaration describes something the chart no longer
        // produces.
        if !pairs.is_empty() && !rendered_anywhere {
            report.fail(
                &at,
                format!(
                    "the selector {} matches no {} in any of the {} rendered values files, so this \
                     document is declared but never produced; correct the selector, or drop the \
                     document if the component is gone",
                    selector_text(&document.source.selector),
                    document.source.kind,
                    pairs.len()
                ),
            );
        }

        // Same reasoning one level down. A Job that renders only when a feature is switched on is a
        // container missing from one values file; a container missing from *every* one is a name
        // the chart no longer produces.
        if !pairs.is_empty() && rendered_anywhere {
            let mut missing: Vec<&str> = document
                .consumers
                .iter()
                .flat_map(|consumer| consumer.containers.iter())
                .map(String::as_str)
                .filter(|name| !containers_seen.iter().any(|seen| seen == name))
                .collect();
            missing.sort_unstable();
            missing.dedup();
            for name in missing {
                report.fail(
                    &at,
                    format!(
                        "the container {} is declared as a consumer of this document but appears \
                         in none of the {} rendered values files; correct the name, or drop it if \
                         the workload is gone",
                        crate::gate::quoted(name),
                        pairs.len()
                    ),
                );
            }
        }
    }
    Ok(())
}

/// Say once, per document, that the range step was skipped for want of a read table.
///
/// Once per *document* rather than once per value, because it is one fact about the document: the
/// loader whose reads every `text_constraint` in it was measured against is not one this build has
/// a measured table for. Reporting it per value would say the same sentence hundreds of times about
/// one missing field, and a report people learn to scroll past is a report nobody reads.
///
/// A warning, never an error: the tool could not check something, and the tree is not accused of
/// anything. See [`crate::value`] for why performing the check anyway is the expensive mistake.
fn report_unchecked_range(at: &str, union: &Union, report: &mut Report) {
    if reads_for(&union.loader_name).is_some() {
        return;
    }
    // Only worth saying when there is something the range step would have reached. A document of
    // nothing but `text` keys has no range for any loader, so no gap opens.
    let reachable = union
        .keys
        .iter()
        .chain(union.external_env.iter())
        .any(|entry| {
            matches!(
                entry.text("text_form"),
                Some("integer" | "boolean" | "choice")
            )
        });
    if !reachable {
        return;
    }

    let named = if union.loader_name.is_empty() {
        "it names no `producer.loader`".to_owned()
    } else {
        format!(
            "its `producer.loader` is {}, which this build has no measured read table for",
            crate::gate::quoted(&union.loader_name)
        )
    };
    report.add(
        at,
        warning(format!(
            "range not checked: {named}. Every value's *form* was held to the `text_constraint` \
             the document publishes, and no value was read and held to its `constraint`, so a \
             `minimum`, a `maximum` or a document-space `enum` was not reached. Performing that \
             read with another loader's rules would refuse text this one accepts, which stops a \
             deployment that was correct."
        )),
    );
}

/// One rendered values file. Returns whether the document was there, and which containers.
fn check_pair(
    pair: &Path,
    document: &Document,
    binding: &Binding,
    report: &mut Report,
) -> Result<(bool, Vec<String>), Error> {
    let name = file_name(pair);
    let at = format!("{name}: {}", document.name);
    let relaxed = document.relaxed(name.split_once("--").map_or(name, |(_, rest)| rest));

    let text = std::fs::read_to_string(pair).map_err(|e| Error::io(pair.display(), e))?;
    let manifests = load_manifests(&text)?;

    let findings = check_document(&manifests, &document.source, &binding.union, relaxed)?;
    let present = findings.is_some();
    report.extend(&at, findings.unwrap_or_default());
    let mut seen: Vec<String> = Vec::new();

    for consumer in &document.consumers {
        let matched = select(&manifests, &consumer.kind, &consumer.selector);
        if matched.is_empty() {
            // The component is switched off in this values file, which the caller confirms by
            // seeing the document absent too.
            if !present {
                continue;
            }
            // Deliberately a warning, where the reverse below is an error. A document with no
            // reader is a ConfigMap nobody mounts — waste, and worth saying, but it breaks no
            // deployment. A reader with no document is a pod that mounts something which does not
            // exist, which does.
            report.add(
                &at,
                warning(format!(
                    "the consumer selector {} matches no {}, but the {} it reads was rendered; \
                     nothing in this values file consumes that configuration",
                    selector_text(&consumer.selector),
                    consumer.kind,
                    document.source.kind
                )),
            );
            continue;
        }
        if !present {
            report.fail(
                &at,
                format!(
                    "the consumer selector {} matches a {}, but the {} it reads was not rendered; \
                     the workload would mount configuration that does not exist",
                    selector_text(&consumer.selector),
                    consumer.kind,
                    document.source.kind
                ),
            );
            continue;
        }

        // Several workloads may read one document — a migration Job and two seed Jobs mounting one
        // bootstrap ConfigMap is a real shape — so the selector is allowed to match more than one,
        // and what must hold instead is that every container the declaration names is found in one
        // of them. That is the property that actually catches a renamed container.
        for workload in matched {
            let spec = pod_spec(workload);
            if !relaxed.env {
                report.extend(&at, ServiceLinks::check(spec, &binding.union));
            }
            for found in
                check_containers(&at, &manifests, spec, consumer, binding, relaxed, report)?
            {
                if !seen.contains(&found) {
                    seen.push(found);
                }
            }
        }
    }

    Ok((present, seen))
}

/// The declared containers this workload runs. Returns the ones it had.
fn check_containers(
    at: &str,
    manifests: &[Json],
    spec: &Json,
    consumer: &Consumer,
    binding: &Binding,
    relaxed: Relaxed,
    report: &mut Report,
) -> Result<Vec<String>, Error> {
    let present = containers_of(spec);
    let mut checked = Vec::new();

    for name in &consumer.containers {
        // Not an error here: another workload matching the same selector may run it, and the caller
        // fails on the containers no workload turned out to have.
        let Some((_, container)) = present.iter().find(|(held, _)| held == name) else {
            continue;
        };
        checked.push(name.clone());

        // The container's own image decides which contract gates 2 and 3 read, so a values file
        // that overrode the image would otherwise have them validated against a contract describing
        // a different binary — silently.
        let reference = container.get("image").and_then(Json::as_str).unwrap_or("");
        let mine = digest_of(reference).and_then(|digest| binding.by_digest.get(digest));
        let Some(mine) = mine else {
            report.fail(
                at,
                format!(
                    "container {} runs {}, which is not one of the images this document declares, \
                     so no contract describes what it reads",
                    crate::gate::quoted(name),
                    if reference.is_empty() {
                        "None".to_owned()
                    } else {
                        crate::gate::quoted(reference)
                    }
                ),
            );
            continue;
        };

        report.extend(
            at,
            check_container(manifests, spec, container, mine, relaxed)?,
        );
    }

    Ok(checked)
}

/// Every rendered values file belonging to one chart, sorted.
fn rendered_pairs(rendered: &Path, chart: &str) -> Result<Vec<std::path::PathBuf>, Error> {
    let opening = format!("{chart}--");
    let mut found: Vec<std::path::PathBuf> = Vec::new();
    for entry in std::fs::read_dir(rendered).map_err(|e| Error::io(rendered.display(), e))? {
        let path = entry.map_err(|e| Error::io(rendered.display(), e))?.path();
        let name = file_name(&path).to_owned();
        // `.yaml` exactly: a render writes the names, and a case-insensitive match here would pick up
        // a `.YAML` a person put there on purpose to keep it out.
        if name.starts_with(&opening)
            && std::path::Path::new(&name).extension() == Some(std::ffi::OsStr::new("yaml"))
        {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

/// One path's file name, or the empty string when it has none.
fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("")
}

/// One YAML file as JSON, empty when the file is not there.
fn read_file(path: &Path) -> Result<Json, Error> {
    if !path.is_file() {
        return Ok(Json::Object(serde_json::Map::new()));
    }
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path.display(), e))?;
    read_yaml(&text, path)
}

/// A selector as a message shows it, in the order the declaration wrote it.
fn selector_text(selector: &[(String, String)]) -> String {
    let fields: Vec<String> = selector
        .iter()
        .map(|(name, value)| {
            format!(
                "{}: {}",
                Json::String(name.clone()),
                Json::String(value.clone())
            )
        })
        .collect();
    format!("{{{}}}", fields.join(", "))
}

/// Whether a chart declares a contract at all, for a caller that only wants the answer.
///
/// # Errors
/// [`Error::Invalid`] when the declaration exists and cannot be read.
pub fn declares(chart_dir: &Path) -> Result<bool, Error> {
    Ok(super::load_declaration(chart_dir)?
        .is_some_and(|declaration| !declaration.documents.is_empty()))
}

/// Where a chart's declaration lives, for a message.
pub fn declaration_path(chart: &str) -> String {
    format!("{chart}: {DECLARATION}")
}

/// Every `(values path, repository)` a chart's values pin an image at.
///
/// Discovered by shape rather than by a list of known paths: anything with a `repository` is an
/// image block, which is what a chart's own image template assumes. A chart that grows a new service
/// is therefore seen the moment its values are added, with nothing here to update.
pub fn pinned_images(values: &Json) -> Vec<(String, String)> {
    fn walk(values: &Json, at: &str, found: &mut Vec<(String, String)>) {
        match values {
            Json::Object(fields) => {
                if let Some(repository) = fields
                    .get("repository")
                    .and_then(Json::as_str)
                    .filter(|repository| !repository.is_empty())
                {
                    found.push((at.to_owned(), repository.to_owned()));
                }
                for (name, value) in fields {
                    let next = if at.is_empty() {
                        name.clone()
                    } else {
                        format!("{at}.{name}")
                    };
                    walk(value, &next, found);
                }
            }
            Json::Array(items) => {
                for (index, value) in items.iter().enumerate() {
                    walk(value, &format!("{at}[{index}]"), found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    walk(values, "", &mut found);
    found
}

/// One values path's value, for a caller that has the tree already.
pub fn at_path<'a>(values: &'a Json, path: &str) -> Option<&'a Json> {
    dig(values, path)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{pinned_images, report_unchecked_range, selector_text};
    use crate::report::{Level, Report};
    use crate::union::union_contracts;

    fn union(loader: Option<&str>, keys: &serde_json::Value) -> crate::union::Union {
        let mut contract = json!({
            "terrace_contract": 1,
            "producer": {"name": "x", "version": "1", "loader": loader.unwrap_or("")},
            "app": {"name": "x"},
            "schema": {
                "schema_version": 2,
                "dialect": {"prefix": "P_", "nesting_separator": "__", "indirection_suffix": "_FILE"},
                "loader": [], "keys": keys.clone(),
            },
            "json_schema": {},
            "external": {"env": [], "ignore": [], "unknown": "reject"},
        });
        if loader.is_none() {
            contract
                .as_object_mut()
                .expect("an object")
                .remove("producer");
        }
        union_contracts(&[("a".to_owned(), contract)]).expect("one contract merges")
    }

    #[test]
    fn a_document_naming_no_loader_says_the_range_was_skipped_once() {
        let mut report = Report::new();
        report_unchecked_range(
            "chart: app",
            &union(None, &json!([{"path": "a", "text_form": "integer"}])),
            &mut report,
        );
        assert_eq!(report.entries().len(), 1);
        assert_eq!(report.entries()[0].finding.level, Level::Warning);
        assert!(
            report.entries()[0]
                .finding
                .message
                .starts_with("range not checked:"),
            "{:?}",
            report.entries()[0]
        );
    }

    #[test]
    fn a_document_of_nothing_but_text_keys_opens_no_gap_and_says_nothing() {
        let mut report = Report::new();
        report_unchecked_range(
            "chart: app",
            &union(None, &json!([{"path": "a", "text_form": "text"}])),
            &mut report,
        );
        assert_eq!(report.entries().len(), 0);
    }

    #[test]
    fn a_known_loader_says_nothing_at_all() {
        let mut report = Report::new();
        report_unchecked_range(
            "chart: app",
            &union(
                Some("figment"),
                &json!([{"path": "a", "text_form": "integer"}]),
            ),
            &mut report,
        );
        assert_eq!(report.entries().len(), 0);
    }

    #[test]
    fn an_image_block_is_found_by_its_shape_rather_than_by_a_list_of_paths() {
        let values = json!({
            "image": {"repository": "ghcr.io/x/y"},
            "services": {"api": {"image": {"repository": "ghcr.io/x/api"}}},
            "nothing": {"here": true},
        });
        let mut found = pinned_images(&values);
        found.sort();
        assert_eq!(
            found,
            vec![
                ("image".to_owned(), "ghcr.io/x/y".to_owned()),
                ("services.api.image".to_owned(), "ghcr.io/x/api".to_owned()),
            ]
        );
    }

    #[test]
    fn a_selector_is_shown_in_the_order_it_was_written() {
        let selector = vec![
            ("b".to_owned(), "2".to_owned()),
            ("a".to_owned(), "1".to_owned()),
        ];
        assert_eq!(selector_text(&selector), r#"{"b": "2", "a": "1"}"#);
    }
}
