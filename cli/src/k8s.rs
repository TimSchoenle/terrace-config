//! Navigating rendered Kubernetes manifests.
//!
//! **Nothing here knows what a configuration contract is, and nothing here knows what a chart is.**
//! It answers the questions the gates ask of a rendered tree (which object does this selector name,
//! which containers does that workload run, what does this container mount, and what file names
//! will appear there), and it answers them the same way for every tree, so a rule never grows a
//! special case for one.
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

use std::collections::{BTreeMap, BTreeSet};

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

/// The rendered objects holding a configuration document: `kind` objects carrying every label in
/// `selector`, narrowed to those holding `key` when the labels match more than one.
///
/// A selector is usually the release label, which a chart sets on every object it renders, so a
/// chart with a second `ConfigMap` (scripts, dashboards, a proxy's configuration) would otherwise
/// make every document ambiguous. The key is part of what names the document, so it breaks the tie.
/// A single match is returned whether it holds the key or not, so the caller can say the key is
/// missing rather than that nothing matched; several matches none of which holds it are returned
/// unnarrowed, so the caller can say the selector is ambiguous.
pub fn select_source<'a>(
    manifests: &'a [Json],
    kind: &str,
    selector: &[(String, String)],
    key: &str,
) -> Vec<&'a Json> {
    let matched = select(manifests, kind, selector);
    if matched.len() < 2 {
        return matched;
    }
    let holding: Vec<&Json> = matched
        .iter()
        .copied()
        .filter(|object| {
            ["data", "stringData"].iter().any(|section| {
                object
                    .get(*section)
                    .and_then(Json::as_object)
                    .is_some_and(|data| data.contains_key(key))
            })
        })
        .collect();
    // None holding the key leaves the ambiguity for the caller to report, rather than an empty
    // match that reads as "not rendered" and skips the document.
    if holding.is_empty() { matched } else { holding }
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

/// Every container by name, init containers included, since they read the same configuration.
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
///
/// `envFrom` is read too, and it has to be: a `secretRef` handing a container every key of a Secret
/// is how a chart commonly supplies credentials, and a reader that skipped it would call those keys
/// unsupplied and every variable they carry unclassified. Each source is followed back to the
/// object `manifests` renders, and contributes one variable per key, its `prefix` prepended. The
/// precedence is the kubelet's: a later source overrides an earlier one, and `env` overrides them
/// all. A `ConfigMap`'s values are visible; a Secret's are not read, the same as a `valueFrom`, so a
/// credential's bytes never reach a finding. A source naming an object the render does not hold
/// (an existing Secret, an operator's output) contributes nothing it can enumerate, and is recorded
/// in [`Environment::unresolved`] so a caller can say what it could not see.
pub fn environment_of(manifests: &[Json], container: &Json) -> Environment {
    let mut environment = Environment::default();

    for source in container
        .get("envFrom")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let prefix = source.get("prefix").and_then(Json::as_str).unwrap_or("");
        let (kind, reference) = if let Some(reference) = source.get("configMapRef") {
            ("ConfigMap", reference)
        } else if let Some(reference) = source.get("secretRef") {
            ("Secret", reference)
        } else {
            continue;
        };
        let Some(name) = reference.get("name").and_then(Json::as_str) else {
            continue;
        };
        let origin = format!("envFrom {kind} {}", crate::text::quoted(name));
        let Some(object) = rendered_object(manifests, kind, name) else {
            environment.unresolved.push(origin);
            continue;
        };
        for (key, value) in env_from_entries(object, kind) {
            let variable = format!("{prefix}{key}");
            environment.origins.insert(variable.clone(), origin.clone());
            environment.set(variable, value);
        }
    }

    for entry in container
        .get("env")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = entry.get("name").and_then(Json::as_str) else {
            continue;
        };
        environment.origins.remove(name);
        environment.set(name.to_owned(), entry.get("value").map(scalar));
    }
    environment
}

/// The rendered object `kind` called `name`, if the render holds one.
fn rendered_object<'a>(manifests: &'a [Json], kind: &str, name: &str) -> Option<&'a Json> {
    manifests.iter().find(|manifest| {
        manifest.get("kind").and_then(Json::as_str) == Some(kind)
            && manifest
                .get("metadata")
                .and_then(|metadata| metadata.get("name"))
                .and_then(Json::as_str)
                == Some(name)
    })
}

/// The keys one `envFrom` object hands a container, each with its value when that is readable.
///
/// A `ConfigMap`'s `data` is plain text. A Secret's `data` and `stringData` are both credentials,
/// so their keys are returned and their values are not. `binaryData` is not handed to an
/// environment at all.
fn env_from_entries(object: &Json, kind: &str) -> Vec<(String, Option<String>)> {
    let sections: &[&str] = if kind == "Secret" {
        &["data", "stringData"]
    } else {
        &["data"]
    };
    let mut entries = Vec::new();
    for section in sections {
        for (key, value) in object
            .get(*section)
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            let value = (kind == "ConfigMap").then(|| scalar(value));
            entries.push((key.clone(), value));
        }
    }
    entries
}

/// One container's environment, as the manifest states it.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// Every variable and its value, in the order the kubelet would settle them. A variable whose
    /// value is not visible holds the empty string.
    pub values: Vec<(String, String)>,
    /// The variables whose value is supplied at run time and cannot be read from the manifest.
    pub opaque: BTreeSet<String>,
    /// The `envFrom` source each variable came from, for a variable `env` did not set itself.
    pub origins: BTreeMap<String, String>,
    /// The `envFrom` sources naming an object the render does not hold, as `envFrom Secret 'x'`.
    pub unresolved: Vec<String>,
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

    /// Sets one variable, replacing an earlier setting of it; [`None`] is a value not visible.
    fn set(&mut self, name: String, value: Option<String>) {
        if value.is_some() {
            self.opaque.remove(&name);
        } else {
            self.opaque.insert(name.clone());
        }
        let value = value.unwrap_or_default();
        match self.values.iter_mut().find(|(held, _)| *held == name) {
            Some(held) => held.1 = value,
            None => self.values.push((name, value)),
        }
    }
}

/// A manifest scalar as the text a container would receive.
///
/// A rendered `value:` is a string in every well-formed manifest, but a chart that wrote a bare
/// number produces one here, and rendering it as `123` rather than refusing it is what lets the
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

/// The kinds whose objects run a pod spec.
const WORKLOAD_KINDS: [&str; 7] = [
    "CronJob",
    "DaemonSet",
    "Deployment",
    "Job",
    "Pod",
    "ReplicaSet",
    "StatefulSet",
];

/// One workload that mounts a given object, and the containers of it that do.
#[derive(Debug, Clone, PartialEq)]
pub struct Mounting<'a> {
    /// The workload, as rendered.
    pub workload: &'a Json,
    /// The containers that mount a volume presenting the object, init containers first.
    pub containers: Vec<&'a str>,
}

/// Every workload whose pod spec mounts the `ConfigMap` or Secret called `name`, in manifest order.
///
/// Followed through a volume, plain or projected, to the containers that mount that volume. A pod
/// that declares the volume and mounts it in no container reads nothing from it, so it is not
/// returned; neither is a reference by environment, which hands a container single values rather
/// than the document.
#[must_use]
pub fn mounting<'a>(manifests: &'a [Json], kind: &str, name: &str) -> Vec<Mounting<'a>> {
    let mut found = Vec::new();
    for workload in manifests {
        let is_workload = workload
            .get("kind")
            .and_then(Json::as_str)
            .is_some_and(|held| WORKLOAD_KINDS.contains(&held));
        if !is_workload {
            continue;
        }
        let spec = pod_spec(workload);
        let volumes: BTreeSet<&str> = spec
            .get("volumes")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter(|volume| presents(volume, kind, name))
            .filter_map(|volume| volume.get("name").and_then(Json::as_str))
            .collect();
        if volumes.is_empty() {
            continue;
        }
        let containers: Vec<&str> = containers_of(spec)
            .into_iter()
            .filter(|(_, container)| {
                container
                    .get("volumeMounts")
                    .and_then(Json::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|mount| mount.get("name").and_then(Json::as_str))
                    .any(|mounted| volumes.contains(mounted))
            })
            .map(|(held, _)| held)
            .collect();
        if !containers.is_empty() {
            found.push(Mounting {
                workload,
                containers,
            });
        }
    }
    found
}

/// Whether one volume presents the object `kind` / `name`, directly or as a projected source.
fn presents(volume: &Json, kind: &str, name: &str) -> bool {
    let (field, name_key) = match kind {
        "ConfigMap" => ("configMap", "name"),
        "Secret" => ("secret", "secretName"),
        _ => return false,
    };
    let direct = volume
        .get(field)
        .and_then(|source| source.get(name_key))
        .and_then(Json::as_str)
        == Some(name);
    let projected = volume
        .get("projected")
        .and_then(|projected| projected.get("sources"))
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .any(|source| {
            source
                .get(field)
                .and_then(|object| object.get("name"))
                .and_then(Json::as_str)
                == Some(name)
        });
    direct || projected
}

/// The file names one volume actually presents, following it back to the rendered object.
///
/// A projected volume of secret sources with explicit `items` names them right there. A source
/// *without* `items` presents every key of the object it names, which is why the rendered Secret is
/// looked up rather than assumed empty: a chart pointing at an existing Secret is the case that
/// would otherwise go unchecked.
pub fn projected_file_names(manifests: &[Json], spec: &Json, volume_name: &str) -> Vec<String> {
    file_names(
        manifests,
        spec,
        volume_name,
        &[("secret", "Secret"), ("configMap", "ConfigMap")],
    )
}

/// The file names one volume presents *from Secret sources only*.
///
/// [`projected_file_names`] answers the neighbouring question and deliberately merges Secret and
/// `ConfigMap` sources, because a gate asking whether a name spells a key does not care where the
/// bytes came from. A caller reasoning about a credential's blast radius does: `config.toml`
/// arriving from a `ConfigMap` in the same projected volume is not one. Same walk, one source kind.
pub fn secret_file_names(manifests: &[Json], spec: &Json, volume_name: &str) -> Vec<String> {
    file_names(manifests, spec, volume_name, &[("secret", "Secret")])
}

/// The sources of one volume whose file names the render cannot list, as a message names them.
///
/// [`projected_file_names`] answers from what the render shows, and is silent about the rest: a
/// `Secret` or `ConfigMap` source without `items` naming an object the render does not hold (an
/// existing Secret, an operator's output), or a volume of another type altogether (a CSI driver, an
/// `emptyDir` an init container fills). A caller that concludes "no file supplies this" from that
/// silence would be wrong, so this says what it could not see.
pub fn unlisted_sources(manifests: &[Json], spec: &Json, volume_name: &str) -> Vec<String> {
    let Some((volume, sources)) = volume_sources(spec, volume_name) else {
        return Vec::new();
    };
    let listable = ["projected", "secret", "configMap"];
    if !listable.iter().any(|field| volume.get(*field).is_some()) {
        return vec![format!("volume {}", crate::text::quoted(volume_name))];
    }
    let mut unlisted = Vec::new();
    for source in &sources {
        for (field, kind) in [("secret", "Secret"), ("configMap", "ConfigMap")] {
            let Some(body) = source.get(field) else {
                continue;
            };
            let itemised = body
                .get("items")
                .and_then(Json::as_array)
                .is_some_and(|items| !items.is_empty());
            let Some(name) = body.get("name").and_then(Json::as_str) else {
                continue;
            };
            if !itemised && rendered_object(manifests, kind, name).is_none() {
                unlisted.push(format!("{kind} {}", crate::text::quoted(name)));
            }
        }
    }
    unlisted
}

/// One volume of `spec`, and its `Secret` and `ConfigMap` sources in the projected shape.
fn volume_sources<'a>(spec: &'a Json, volume_name: &str) -> Option<(&'a Json, Vec<Json>)> {
    let volume = spec
        .get("volumes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .find(|volume| volume.get("name").and_then(Json::as_str) == Some(volume_name))?;

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
    Some((volume, sources))
}

/// One volume's presented file names, restricted to the source kinds the caller asked for.
fn file_names(
    manifests: &[Json],
    spec: &Json,
    volume_name: &str,
    kinds: &[(&str, &str)],
) -> Vec<String> {
    let Some((_, sources)) = volume_sources(spec, volume_name) else {
        return Vec::new();
    };

    let mut names: BTreeSet<String> = BTreeSet::new();
    for source in &sources {
        for &(field, kind) in kinds {
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
    use serde_json::{Value as Json, json};

    use super::{
        containers_of, digest_of, environment_of, inside_a_mount, load_manifests, mounting,
        pod_spec, projected_file_names, select, select_source, unlisted_sources, volume_at,
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
        let environment = environment_of(&[], &container);
        assert!(environment.visible("A"));
        assert!(!environment.visible("B"));
        assert_eq!(environment.sorted(), vec![("A", "1"), ("B", "")]);
    }

    #[test]
    fn env_from_hands_over_every_key_of_the_rendered_object_and_env_overrides_it() {
        let manifests = vec![
            json!({"kind": "ConfigMap", "metadata": {"name": "settings"},
                   "data": {"A": "from-config", "B": "1"}}),
            json!({"kind": "Secret", "metadata": {"name": "creds"},
                   "data": {"TOKEN": "c2VjcmV0"}, "stringData": {"OTHER": "x"}}),
        ];
        let container = json!({
            "envFrom": [
                {"configMapRef": {"name": "settings"}},
                {"secretRef": {"name": "creds"}, "prefix": "P_"},
            ],
            "env": [{"name": "A", "value": "from-env"}],
        });
        let environment = environment_of(&manifests, &container);
        assert_eq!(
            environment.sorted(),
            vec![
                ("A", "from-env"),
                ("B", "1"),
                ("P_OTHER", ""),
                ("P_TOKEN", "")
            ]
        );
        // A Secret's bytes are never read; a ConfigMap's are.
        assert!(environment.visible("B"));
        assert!(!environment.visible("P_TOKEN"));
        assert_eq!(environment.origins.get("A"), None);
        assert_eq!(
            environment.origins.get("P_TOKEN").map(String::as_str),
            Some("envFrom Secret 'creds'")
        );
        assert_eq!(environment.unresolved, Vec::<String>::new());
    }

    #[test]
    fn a_later_env_from_source_overrides_an_earlier_one() {
        let manifests = vec![
            json!({"kind": "ConfigMap", "metadata": {"name": "a"}, "data": {"X": "first"}}),
            json!({"kind": "Secret", "metadata": {"name": "b"}, "data": {"X": "c2Vjb25k"}}),
        ];
        let container = json!({"envFrom": [
            {"configMapRef": {"name": "a"}},
            {"secretRef": {"name": "b"}},
        ]});
        let environment = environment_of(&manifests, &container);
        assert_eq!(environment.sorted(), vec![("X", "")]);
        assert!(!environment.visible("X"));
        assert_eq!(
            environment.origins.get("X").map(String::as_str),
            Some("envFrom Secret 'b'")
        );
    }

    #[test]
    fn an_env_from_source_the_render_does_not_hold_is_recorded_rather_than_guessed() {
        let container = json!({"envFrom": [{"secretRef": {"name": "external", "optional": true}}]});
        let environment = environment_of(&[], &container);
        assert_eq!(environment.values, Vec::new());
        assert_eq!(environment.unresolved, vec!["envFrom Secret 'external'"]);
    }

    #[test]
    fn a_source_the_render_cannot_list_is_named_rather_than_read_as_empty() {
        let manifests = vec![json!({"kind": "Secret", "metadata": {"name": "rendered"}})];
        let spec = json!({"volumes": [
            {"name": "mixed", "projected": {"sources": [
                {"secret": {"name": "rendered"}},
                {"secret": {"name": "external"}},
                {"secret": {"name": "listed", "items": [{"key": "a", "path": "a"}]}},
                {"configMap": {"name": "settings"}},
            ]}},
            {"name": "csi", "csi": {"driver": "secrets-store.csi.k8s.io"}},
        ]});
        assert_eq!(
            unlisted_sources(&manifests, &spec, "mixed"),
            vec!["Secret 'external'", "ConfigMap 'settings'"]
        );
        assert_eq!(
            unlisted_sources(&manifests, &spec, "csi"),
            vec!["volume 'csi'"]
        );
        assert_eq!(
            unlisted_sources(&manifests, &spec, "absent"),
            Vec::<String>::new()
        );
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
    fn a_workload_mounts_an_object_through_a_plain_or_a_projected_volume() {
        let manifests = vec![
            json!({"kind": "Deployment", "metadata": {"name": "web"}, "spec": {"template": {
                "spec": {
                    "volumes": [{"name": "config", "configMap": {"name": "app"}}],
                    "initContainers": [{"name": "wait"}],
                    "containers": [
                        {"name": "web", "volumeMounts": [{"name": "config", "mountPath": "/c"}]},
                        {"name": "sidecar"},
                    ],
                },
            }}}),
            json!({"kind": "CronJob", "metadata": {"name": "sweep"}, "spec": {"jobTemplate": {
                "spec": {"template": {"spec": {
                    "volumes": [{"name": "all", "projected": {"sources": [
                        {"configMap": {"name": "app"}},
                    ]}}],
                    "containers": [
                        {"name": "sweep", "volumeMounts": [{"name": "all", "mountPath": "/c"}]},
                    ],
                }}},
            }}}),
            json!({"kind": "ConfigMap", "metadata": {"name": "app"}}),
        ];
        let found = mounting(&manifests, "ConfigMap", "app");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].workload["metadata"]["name"], "web");
        assert_eq!(found[0].containers, vec!["web"]);
        assert_eq!(found[1].containers, vec!["sweep"]);
    }

    #[test]
    fn a_volume_no_container_mounts_and_another_objects_volume_are_not_mounting() {
        let manifests = vec![
            json!({"kind": "Deployment", "metadata": {"name": "idle"}, "spec": {"template": {
                "spec": {
                    "volumes": [{"name": "config", "configMap": {"name": "app"}}],
                    "containers": [{"name": "idle"}],
                },
            }}}),
            json!({"kind": "Deployment", "metadata": {"name": "other"}, "spec": {"template": {
                "spec": {
                    "volumes": [{"name": "config", "configMap": {"name": "elsewhere"}}],
                    "containers": [
                        {"name": "other", "volumeMounts": [{"name": "config", "mountPath": "/c"}]},
                    ],
                },
            }}}),
        ];
        assert!(
            mounting(&manifests, "ConfigMap", "app").is_empty(),
            "neither workload mounts `app`"
        );
    }

    #[test]
    fn a_secret_is_named_by_secret_name_in_a_volume_and_by_name_in_a_projection() {
        let manifests = vec![
            json!({"kind": "StatefulSet", "metadata": {"name": "db"}, "spec": {
                "template": {"spec": {
                    "volumes": [
                        {"name": "a", "secret": {"secretName": "creds"}},
                        {"name": "b", "projected": {"sources": [{"secret": {"name": "creds"}}]}},
                    ],
                    "containers": [
                        {"name": "one", "volumeMounts": [{"name": "a", "mountPath": "/a"}]},
                        {"name": "two", "volumeMounts": [{"name": "b", "mountPath": "/b"}]},
                    ],
                }},
            }}),
        ];
        let found = mounting(&manifests, "Secret", "creds");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].containers, vec!["one", "two"]);
    }

    #[test]
    fn a_reference_without_a_digest_pins_nothing() {
        assert_eq!(digest_of("ghcr.io/x/y:v1@sha256:ab"), Some("sha256:ab"));
        assert_eq!(digest_of("ghcr.io/x/y:v1"), None);
    }

    #[test]
    fn a_source_is_narrowed_by_its_key_only_when_its_labels_are_ambiguous() {
        let labelled = |name: &str, data: Json| {
            json!({"kind": "ConfigMap", "metadata": {"name": name, "labels": {"a": "b"}},
                   "data": data})
        };
        let selector = [("a".to_owned(), "b".to_owned())];
        let names = |found: Vec<&Json>| -> Vec<String> {
            found
                .iter()
                .map(|object| object["metadata"]["name"].to_string())
                .collect()
        };

        let ambiguous = [
            labelled("config", json!({"config.toml": ""})),
            labelled("scripts", json!({"run.sh": ""})),
        ];
        assert_eq!(
            names(select_source(
                &ambiguous,
                "ConfigMap",
                &selector,
                "config.toml"
            )),
            vec!["\"config\"".to_owned()]
        );

        let none_hold_it = [
            labelled("scripts", json!({"run.sh": ""})),
            labelled("dashboards", json!({"a.json": ""})),
        ];
        assert_eq!(
            select_source(&none_hold_it, "ConfigMap", &selector, "config.toml").len(),
            2,
            "no holder leaves the ambiguity to be reported, not an empty match"
        );

        let single = [labelled("scripts", json!({"run.sh": ""}))];
        assert_eq!(
            names(select_source(
                &single,
                "ConfigMap",
                &selector,
                "config.toml"
            )),
            vec!["\"scripts\"".to_owned()],
            "one match is kept, so the caller can say the key is missing"
        );

        let secret = [
            json!({"kind": "Secret", "metadata": {"name": "s", "labels": {"a": "b"}},
                   "stringData": {"config.toml": ""}}),
            json!({"kind": "Secret", "metadata": {"name": "t", "labels": {"a": "b"}}}),
        ];
        assert_eq!(
            names(select_source(&secret, "Secret", &selector, "config.toml")),
            vec!["\"s\"".to_owned()]
        );
    }
}
