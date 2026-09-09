//! Navigating rendered Kubernetes manifests.
//!
//! **Nothing here knows what a configuration contract is, and nothing here knows what a chart is.**
//! It answers the questions the gates ask of a rendered tree — which object does this selector name,
//! which containers does that workload run, what does this container mount, and what file names will
//! appear there — and it answers them the same way for every tree, so a rule never grows a special
//! case for one.
//!
//! That is also the mechanical claim the feature stack makes: `helm` sits above this, so a Helm
//! concept cannot reach a rule here without failing to compile in the feature set that leaves `helm`
//! out.
//!
//! # Manifests are read, never rendered
//!
//! Rendering belongs to whatever produced the tree. The gates must read byte-identical manifests to
//! the ones the rest of a pipeline validates, and a second renderer with its own flags is a second
//! answer to what the chart produces.

use std::collections::BTreeSet;

use serde::Deserialize as _;
use serde_json::{Map, Value as Json};

use crate::error::Error;

/// Every mapping document in one rendered YAML file, in order.
///
/// Non-mappings are dropped rather than refused: a rendered file legitimately carries an empty
/// document wherever a template's whole body was switched off by its values.
///
/// # Errors
/// [`Error::Invalid`] when the text is not YAML at all.
pub fn load_manifests(text: &str) -> Result<Vec<Json>, Error> {
    let mut found = Vec::new();
    for document in serde_norway::Deserializer::from_str(text) {
        let value = Json::deserialize(document)
            .map_err(|failure| Error::Invalid(format!("not valid YAML: {failure}")))?;
        if value.is_object() {
            found.push(value);
        }
    }
    Ok(found)
}

/// Every object of `kind` whose labels are a superset of `selector`.
///
/// Matched by label rather than by rendered name: a chart's name override moves the name of every
/// object it creates, so a selector written against one would silently match nothing under a values
/// file that sets it. A selector matching zero or several objects is reported by the caller rather
/// than resolved here, which is what keeps a stale selector from turning into a skipped check.
pub fn select<'a>(
    manifests: &'a [Json],
    kind: &str,
    selector: &[(String, String)],
) -> Vec<&'a Json> {
    manifests
        .iter()
        .filter(|manifest| manifest.get("kind").and_then(Json::as_str) == Some(kind))
        .filter(|manifest| {
            let labels = manifest
                .get("metadata")
                .and_then(|metadata| metadata.get("labels"));
            selector.iter().all(|(name, value)| {
                labels
                    .and_then(|labels| labels.get(name))
                    .and_then(Json::as_str)
                    == Some(value.as_str())
            })
        })
        .collect()
}

/// The names of several matched objects, for a message that has to say which.
pub fn names_of(manifests: &[&Json]) -> String {
    manifests
        .iter()
        .map(|manifest| {
            manifest
                .get("metadata")
                .and_then(|metadata| metadata.get("name"))
                .and_then(Json::as_str)
                .unwrap_or("?")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The pod spec of a Deployment, `StatefulSet`, Job or `CronJob`, whichever nesting it uses.
pub fn pod_spec(workload: &Json) -> &Json {
    let Some(spec) = workload.get("spec") else {
        return workload;
    };
    let Some(template) = spec.get("template").or_else(|| spec.get("jobTemplate")) else {
        return spec;
    };
    // A CronJob nests one level further: `spec.jobTemplate.spec.template.spec`.
    let template = template
        .get("spec")
        .and_then(|inner| inner.get("template"))
        .unwrap_or(template);
    template.get("spec").unwrap_or(spec)
}

/// Every container by name, init containers included — they read the same configuration.
///
/// In the order the spec lists them, init containers first, so a report is ordered by the manifest
/// rather than by whatever a hash map returned.
pub fn containers_of(spec: &Json) -> Vec<(&str, &Json)> {
    let mut found: Vec<(&str, &Json)> = Vec::new();
    for group in ["initContainers", "containers"] {
        for container in spec
            .get(group)
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            let Some(name) = container.get("name").and_then(Json::as_str) else {
                continue;
            };
            match found.iter_mut().find(|(held, _)| *held == name) {
                Some(held) => held.1 = container,
                None => found.push((name, container)),
            }
        }
    }
    found
}

/// A container's environment: the variables in spec order, and the ones whose value is not visible.
///
/// A `valueFrom` entry is a name a rule can classify and a value it cannot see until the pod runs.
/// Both halves are returned so a caller checks the spelling and skips the value, rather than either
/// ignoring the variable or checking an empty string against a constraint.
pub fn environment_of(container: &Json) -> Environment {
    let mut values: Vec<(String, String)> = Vec::new();
    let mut opaque: BTreeSet<String> = BTreeSet::new();
    for entry in container
        .get("env")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = entry.get("name").and_then(Json::as_str) else {
            continue;
        };
        let value = if let Some(value) = entry.get("value") {
            scalar(value)
        } else {
            opaque.insert(name.to_owned());
            String::new()
        };
        match values.iter_mut().find(|(held, _)| held == name) {
            Some(held) => held.1 = value,
            None => values.push((name.to_owned(), value)),
        }
    }
    Environment { values, opaque }
}

/// One container's environment, as the manifest states it.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// Every variable and its value, in spec order. A variable whose value is not visible holds the
    /// empty string.
    pub values: Vec<(String, String)>,
    /// The variables whose value is supplied at run time and cannot be read from the manifest.
    pub opaque: BTreeSet<String>,
}

impl Environment {
    /// Whether a value is readable from the manifest, rather than a run-time `valueFrom`.
    pub fn visible(&self, variable: &str) -> bool {
        !self.opaque.contains(variable)
    }

    /// Every variable and value, sorted by name.
    ///
    /// What the rules that report per variable iterate, so a report is ordered by the variable's
    /// own name rather than by where a template happened to emit it.
    pub fn sorted(&self) -> Vec<(&str, &str)> {
        let mut sorted: Vec<(&str, &str)> = self
            .values
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        sorted.sort_unstable();
        sorted
    }
}

/// A manifest scalar as the text a container would receive.
///
/// A rendered `value:` is a string in every well-formed manifest, but a chart that wrote a bare
/// number produces one here — and rendering it as `123` rather than refusing it is what lets the
/// value be checked at all.
fn scalar(value: &Json) -> String {
    match value {
        Json::String(text) => text.clone(),
        Json::Null => String::new(),
        other => other.to_string(),
    }
}

/// Every path this container mounts something at.
pub fn mount_paths(container: &Json) -> Vec<&str> {
    container
        .get("volumeMounts")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|mount| mount.get("mountPath").and_then(Json::as_str))
        .collect()
}

/// The volume mounted exactly at `path`, which is what makes its file names readable.
pub fn volume_at<'a>(container: &'a Json, path: &str) -> Option<&'a str> {
    container
        .get("volumeMounts")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .find(|mount| {
            mount
                .get("mountPath")
                .and_then(Json::as_str)
                .map(|mounted| mounted.trim_end_matches('/'))
                == Some(path.trim_end_matches('/'))
        })
        .and_then(|mount| mount.get("name").and_then(Json::as_str))
}

/// Whether `path` names something under a volume this container actually mounts.
pub fn inside_a_mount(container: &Json, path: &str) -> bool {
    mount_paths(container).into_iter().any(|mounted| {
        path == mounted || path.starts_with(&format!("{}/", mounted.trim_end_matches('/')))
    })
}

/// The file names one volume actually presents, following it back to the rendered object.
///
/// A projected volume of secret sources with explicit `items` names them right there. A source
/// *without* `items` presents every key of the object it names, which is why the rendered Secret is
/// looked up rather than assumed empty — a chart pointing at an existing Secret is the case that
/// would otherwise go unchecked.
pub fn projected_file_names(manifests: &[Json], spec: &Json, volume_name: &str) -> Vec<String> {
    let Some(volume) = spec
        .get("volumes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .find(|volume| volume.get("name").and_then(Json::as_str) == Some(volume_name))
    else {
        return Vec::new();
    };

    let sources: Vec<Json> = if let Some(projected) = volume.get("projected") {
        projected
            .get("sources")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default()
    } else if let Some(secret) = volume.get("secret").and_then(Json::as_object) {
        // A bare `secret` volume names the object in `secretName` where a projected source names it
        // in `name`, so it is rewritten into the projected shape rather than handled twice below.
        let mut body = secret.clone();
        if let Some(name) = secret.get("secretName").cloned() {
            body.insert("name".to_owned(), name);
        }
        let mut source = Map::new();
        source.insert("secret".to_owned(), Json::Object(body));
        vec![Json::Object(source)]
    } else if let Some(config_map) = volume.get("configMap") {
        let mut source = Map::new();
        source.insert("configMap".to_owned(), config_map.clone());
        vec![Json::Object(source)]
    } else {
        Vec::new()
    };

    let mut names: BTreeSet<String> = BTreeSet::new();
    for source in &sources {
        for (field, kind) in [("secret", "Secret"), ("configMap", "ConfigMap")] {
            let Some(body) = source.get(field).and_then(Json::as_object) else {
                continue;
            };
            let items = body.get("items").and_then(Json::as_array);
            if let Some(items) = items.filter(|items| !items.is_empty()) {
                for item in items {
                    // `path` is where the file lands and `key` is where its bytes come from, so a
                    // source that renames on the way in is read by the name it will actually have.
                    if let Some(presented) = item
                        .get("path")
                        .or_else(|| item.get("key"))
                        .and_then(Json::as_str)
                    {
                        names.insert(presented.to_owned());
                    }
                }
                continue;
            }
            for manifest in manifests {
                let matches = manifest.get("kind").and_then(Json::as_str) == Some(kind)
                    && manifest
                        .get("metadata")
                        .and_then(|metadata| metadata.get("name"))
                        == body.get("name");
                if !matches {
                    continue;
                }
                for section in ["data", "stringData"] {
                    for key in manifest
                        .get(section)
                        .and_then(Json::as_object)
                        .into_iter()
                        .flatten()
                    {
                        names.insert(key.0.clone());
                    }
                }
            }
        }
    }
    names.into_iter().collect()
}

/// The `sha256:...` a rendered container image pins, if it pins one.
pub fn digest_of(reference: &str) -> Option<&str> {
    reference.split_once('@').map(|(_, digest)| digest)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        containers_of, digest_of, environment_of, inside_a_mount, load_manifests, pod_spec,
        projected_file_names, select, volume_at,
    };

    #[test]
    fn an_empty_document_is_dropped_rather_than_refused() {
        // A template whose whole body was switched off by its values renders one of these.
        let manifests = load_manifests("---\nkind: ConfigMap\n---\n---\n# nothing\n")
            .expect("the file is YAML");
        assert_eq!(manifests.len(), 1);
    }

    #[test]
    fn a_selector_matches_on_labels_rather_than_names() {
        let manifests = vec![
            json!({"kind": "ConfigMap", "metadata": {"name": "renamed", "labels": {"c": "api"}}}),
            json!({"kind": "ConfigMap", "metadata": {"name": "other", "labels": {"c": "web"}}}),
        ];
        let matched = select(
            &manifests,
            "ConfigMap",
            &[("c".to_owned(), "api".to_owned())],
        );
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0]["metadata"]["name"], "renamed");
    }

    #[test]
    fn a_cronjob_nests_two_templates_deep() {
        let workload = json!({"spec": {"jobTemplate": {"spec": {"template": {
            "spec": {"containers": [{"name": "run"}]}
        }}}}});
        let spec = pod_spec(&workload);
        assert_eq!(containers_of(spec).len(), 1);
    }

    #[test]
    fn init_containers_are_read_and_come_first() {
        let spec = json!({
            "initContainers": [{"name": "migrate"}],
            "containers": [{"name": "app"}],
        });
        let found = containers_of(&spec);
        assert_eq!(found[0].0, "migrate");
        assert_eq!(found[1].0, "app");
    }

    #[test]
    fn a_value_from_is_a_name_without_a_value() {
        let container = json!({"env": [
            {"name": "A", "value": "1"},
            {"name": "B", "valueFrom": {"secretKeyRef": {"name": "s", "key": "k"}}},
        ]});
        let environment = environment_of(&container);
        assert!(environment.visible("A"));
        assert!(!environment.visible("B"));
        assert_eq!(environment.sorted(), vec![("A", "1"), ("B", "")]);
    }

    #[test]
    fn a_path_under_a_mount_is_inside_it_and_a_prefix_of_one_is_not() {
        let container = json!({"volumeMounts": [{"name": "v", "mountPath": "/secrets"}]});
        assert!(inside_a_mount(&container, "/secrets"));
        assert!(inside_a_mount(&container, "/secrets/token"));
        assert!(!inside_a_mount(&container, "/secretstore"));
        assert_eq!(volume_at(&container, "/secrets/"), Some("v"));
        assert_eq!(volume_at(&container, "/secrets/token"), None);
    }

    #[test]
    fn a_source_without_items_presents_every_key_of_the_object_it_names() {
        // The existing-Secret case, which would otherwise go unchecked entirely.
        let manifests = vec![json!({
            "kind": "Secret", "metadata": {"name": "creds"},
            "data": {"github__token": "eA==", "s3__secret": "eA=="},
        })];
        let spec = json!({"volumes": [
            {"name": "v", "projected": {"sources": [{"secret": {"name": "creds"}}]}}
        ]});
        assert_eq!(
            projected_file_names(&manifests, &spec, "v"),
            vec!["github__token".to_owned(), "s3__secret".to_owned()]
        );
    }

    #[test]
    fn explicit_items_name_the_files_directly() {
        let spec = json!({"volumes": [
            {"name": "v", "projected": {"sources": [
                {"secret": {"name": "creds", "items": [{"key": "k", "path": "github__token"}]}}
            ]}}
        ]});
        assert_eq!(
            projected_file_names(&[], &spec, "v"),
            vec!["github__token".to_owned()]
        );
    }

    #[test]
    fn a_reference_without_a_digest_pins_nothing() {
        assert_eq!(digest_of("ghcr.io/x/y:v1@sha256:ab"), Some("sha256:ab"));
        assert_eq!(digest_of("ghcr.io/x/y:v1"), None);
    }
}
