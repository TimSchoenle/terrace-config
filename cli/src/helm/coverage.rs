//! Which charts a contract covers, and which it does not.
//!
//! **Adopting a contract is opt-in.** A chart is covered when it carries a declaration, and a chart
//! without one is simply not covered: this reports it and does not fail. Images adopt the format on
//! their own release schedules, and a gate that failed every chart whose image had not caught up
//! would be red for reasons nobody in the consuming repository can fix — so it would end up
//! disabled, which is worse than absent.
//!
//! Two things are still hard errors, because both are a chart contradicting *itself* rather than
//! waiting on someone else:
//!
//! - a chart that declares documents and leaves one of its own first-party images unaccounted for.
//!   A new service added to a multi-image chart would otherwise be validated by nothing while the
//!   chart still looked covered — the failure this exists to prevent, and the one case where the
//!   chart's author has everything they need to fix it.
//! - a declaration that cannot be read as one.
//!
//! It runs without a render, which is what makes it cheap enough to sit beside the gates that need
//! one.
//!
//! # It ports with its entry point rather than with its phase
//!
//! The plan groups coverage with the marker language, one phase later than the gates. It arrives
//! here because the two shared one entry point: deleting that entry point without this would leave
//! the recipe for it pointing at nothing. A phase that leaves the old implementation running has
//! doubled the number of places the rule lives, so the smaller deviation is to bring the 179 lines
//! forward.

use std::path::Path;

use serde_json::Value as Json;

use crate::classify::matches_ignore;
use crate::error::Error;
use crate::report::{Report, warning};

use super::DECLARATION;
use super::check::pinned_images;
use super::declaration::{Declaration, chart_dirs, load_declaration, read_yaml};

/// What a coverage run found.
#[derive(Debug)]
pub struct Coverage {
    /// Everything it had to say.
    pub report: Report,
    /// The charts that carry a declaration with documents in it.
    pub covered: Vec<String>,
    /// The charts that pin a first-party image and declare nothing.
    pub uncovered: Vec<String>,
}

/// Report which charts are covered by a contract, failing only on a chart that contradicts itself.
///
/// # Errors
/// [`Error::Io`] when the tree or the first-party list cannot be read, [`Error::Invalid`] when a
/// declaration cannot be read as one.
pub fn coverage(charts: &Path, first_party: &Path) -> Result<Coverage, Error> {
    let patterns = first_party_patterns(first_party)?;
    let mut report = Report::new();
    let mut covered: Vec<String> = Vec::new();
    let mut uncovered: Vec<String> = Vec::new();

    for chart_dir in chart_dirs(charts)? {
        // A chart with no values pins no image, so there is nothing here to be covered or
        // uncovered. Checked here rather than in the shared walk because every other caller
        // legitimately reads a chart without one.
        let values_path = chart_dir.join("values.yaml");
        if !values_path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&values_path)
            .map_err(|e| Error::io(values_path.display(), e))?;
        let values = read_yaml(&text, &values_path)?;

        let ours: Vec<(String, String)> = pinned_images(&values)
            .into_iter()
            .filter(|(_, repository)| {
                patterns
                    .iter()
                    .any(|pattern| matches_ignore(pattern, repository))
            })
            .collect();
        if ours.is_empty() {
            continue;
        }

        let name = chart_dir
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let declaration = load_declaration(&chart_dir)?;
        if let Some(declaration) = &declaration {
            check_unconfigured(&name, declaration, &values, &mut report);
        }

        let Some(declaration) = declaration.filter(|held| !held.documents.is_empty()) else {
            report.add(
                &name,
                warning(format!(
                    "pins the first-party image(s) {} and declares no configuration contract, so \
                     its rendered configuration is checked by nothing. Add a {DECLARATION} once \
                     the image publishes one.",
                    ours.iter()
                        .map(|(_, repository)| repository.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            );
            uncovered.push(name);
            continue;
        };

        covered.push(name.clone());

        // From here on the chart has opted in, so an image it does not account for is its own
        // inconsistency rather than a producer that has not shipped yet.
        let mut declared: Vec<&str> = declaration
            .documents
            .iter()
            .flat_map(|document| document.images.iter())
            .map(|reference| reference.values.as_str())
            .collect();
        declared.extend(declaration.unconfigured.iter().map(String::as_str));
        for (path, repository) in &ours {
            if !declared.contains(&path.as_str()) {
                report.fail(
                    super::check::declaration_path(&name),
                    format!(
                        "the values path {} pins the first-party image {repository} and no \
                         document reads it; list it under a document's `images`, or under \
                         `unconfigured` if it reads no contract-described configuration",
                        crate::gate::quoted(path)
                    ),
                );
            }
        }
    }

    Ok(Coverage {
        report,
        covered,
        uncovered,
    })
}

/// Hold every `unconfigured` entry to being a values path that pins an image.
///
/// **`unconfigured` holds values paths, not repository names**, and that had to be settled because
/// two readers of the field disagreed. This unions it with the `values` of every declared image and
/// compares the result against the paths a chart pins, so a repository name there matched nothing
/// and silently bought no coverage.
///
/// The values path wins for three reasons. It is the reading the only gate that acts on the field
/// already implements. That gate's own message offers `unconfigured` as the alternative to "a
/// document's `images`", whose entries are values paths. And a repository name is ambiguous where a
/// values path is not: a chart pinning the same repository at two paths could not say which it
/// meant.
///
/// Enforced here rather than in the declaration reader, which deliberately reads nothing but its own
/// file. This is the one rule that already has both open.
fn check_unconfigured(chart: &str, declaration: &Declaration, values: &Json, report: &mut Report) {
    let pinned: Vec<String> = {
        let mut found: Vec<String> = pinned_images(values)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        found.sort();
        found.dedup();
        found
    };
    for entry in &declaration.unconfigured {
        if pinned.contains(entry) {
            continue;
        }
        report.fail(
            super::check::declaration_path(chart),
            format!(
                "`unconfigured` names {}, which is not a values path this chart pins an image at. \
                 The field holds values paths — the same spelling a document's `images[].values` \
                 uses — and not repository names{}",
                crate::gate::quoted(entry),
                if pinned.is_empty() {
                    String::new()
                } else {
                    format!("; this chart pins images at {}", pinned.join(", "))
                }
            ),
        );
    }
}

/// The repositories an organisation builds, from its own list.
///
/// # Errors
/// [`Error::Invalid`] when the list is missing: coverage cannot be decided without it, and deciding
/// it wrongly would call every chart uncovered.
pub fn first_party_patterns(path: &Path) -> Result<Vec<String>, Error> {
    if !path.is_file() {
        return Err(Error::Invalid(format!(
            "{}: missing; coverage cannot be decided without it",
            path.display()
        )));
    }
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path.display(), e))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::check_unconfigured;
    use crate::helm::declaration::Declaration;
    use crate::report::Report;

    fn declaration(unconfigured: &[&str]) -> Declaration {
        Declaration {
            chart: "x".to_owned(),
            path: std::path::PathBuf::from("x"),
            documents: Vec::new(),
            reason: None,
            unconfigured: unconfigured.iter().map(|held| (*held).to_owned()).collect(),
            bindings: false,
            unbound: Vec::new(),
            credentials: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn unconfigured_holds_a_values_path_and_a_repository_name_is_refused() {
        let values = json!({"image": {"repository": "ghcr.io/x/y"}});
        let mut report = Report::new();
        check_unconfigured("x", &declaration(&["image"]), &values, &mut report);
        assert_eq!(report.entries().len(), 0);

        let mut report = Report::new();
        check_unconfigured("x", &declaration(&["ghcr.io/x/y"]), &values, &mut report);
        assert_eq!(report.entries().len(), 1);
        assert!(
            report.entries()[0]
                .finding
                .message
                .contains("this chart pins images at image"),
            "{:?}",
            report.entries()[0]
        );
    }
}
