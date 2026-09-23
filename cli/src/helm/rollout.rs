//! Whether a rendered workload actually rolls for the changes that need a restart.
//!
//! [`super::restarts`] writes which parts of a document only a restart applies; these gates hold a
//! render to it. Two rules, and each catches a way for a change to be reported as applied and never
//! be:
//!
//! - **A workload rolls for restart keys.** A long-running consumer of a document with keys only a
//!   restart applies carries a `checksum/` pod annotation. Without one, a change to those keys is
//!   written to the mounted file, the rebuilt runtime keeps the boot value, and nothing restarts
//!   the process — which is the shape a chart that assumes every key reloads has today.
//! - **No live key hides behind a mount the kubelet never updates.** A `subPath` mount is resolved
//!   once, at container start, and an `immutable` object cannot change at all. A key the image
//!   applies live, delivered that way, is left out of the restart digest *and* never reaches the
//!   running process. `FORMAT.md` step 7: report it rather than classify it.
//!
//! Both fire only when an image reading the document declares a rebuild. A chart whose images say
//! nothing about reloading rolls on its whole configuration as it always has, and has nothing to
//! answer for here.

use serde_json::Value as Json;

use crate::document::Contract;
use crate::k8s::{digest_of, pod_spec, select};
use crate::reload::{Channel, Effective, classify, rebuilds};
use crate::report::{Finding, error, warning};

use super::declaration::{Binding, Consumer, Document};

/// The workload kinds a rollout applies to. A Job or `CronJob` starts a fresh pod per run, which
/// reads the configuration as it is then.
const LONG_RUNNING: [&str; 3] = ["Deployment", "StatefulSet", "DaemonSet"];

/// What one workload consuming one document owes the reload declarations of its images.
#[must_use]
pub fn check_workload(
    manifests: &[Json],
    workload: &Json,
    consumer: &Consumer,
    document: &Document,
    binding: &Binding,
) -> Vec<Finding> {
    if !binding.contracts.values().any(rebuilds) {
        return Vec::new();
    }
    let kind = workload.get("kind").and_then(Json::as_str).unwrap_or("");
    if !LONG_RUNNING.contains(&kind) {
        return Vec::new();
    }
    let name = workload
        .get("metadata")
        .and_then(|metadata| metadata.get("name"))
        .and_then(Json::as_str)
        .unwrap_or("?");
    let spec = pod_spec(workload);

    let mut findings = Vec::new();
    let mut needs_digest = false;
    for container_name in &consumer.containers {
        let Some((container, init)) = container(spec, container_name) else {
            continue;
        };
        let contract = container
            .get("image")
            .and_then(Json::as_str)
            .and_then(digest_of)
            .and_then(|digest| binding.contracts.get(digest));

        if init {
            needs_digest = true;
            if contract.is_some_and(rebuilds) {
                findings.push(warning(format!(
                    "{kind} {name}: init container `{container_name}` reads {}, and an init \
                     container runs once per pod, so every key it reads needs a restart here — \
                     the pod's checksum has to cover the whole document, not only its restart set",
                    document.name
                )));
            }
            continue;
        }

        let Some(contract) = contract else {
            // An image with no typed contract declares nothing, so everything it reads is restart
            // content; the checksum rule below covers it.
            needs_digest = true;
            continue;
        };
        needs_digest = needs_digest || has_restart_content(contract, document);
        findings.extend(frozen_mounts(
            manifests,
            spec,
            container,
            container_name,
            contract,
            document,
        ));
    }

    if needs_digest && !carries_checksum(workload) {
        findings.push(error(format!(
            "{kind} {name}: {} carries keys only a restart applies, and the pod template has no \
             `checksum/` annotation, so a change to them is written to the mounted file and never \
             applied. Hash the restart set `terrace-contract reload` writes into a pod annotation, \
             or the whole document if this chart rolls on every change.",
            document.name
        )));
    }
    findings
}

/// A container of the pod spec by name, and whether it is an init container.
fn container<'a>(spec: &'a Json, name: &str) -> Option<(&'a Json, bool)> {
    for (group, init) in [("initContainers", true), ("containers", false)] {
        let found = spec
            .get(group)
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .find(|held| held.get("name").and_then(Json::as_str) == Some(name));
        if let Some(found) = found {
            return Some((found, init));
        }
    }
    None
}

/// Whether some key of `contract` is restart content for a change delivered in the document or the
/// chart's Secret — which is to say, whether the pod needs a digest to roll on at all.
fn has_restart_content(contract: &Contract, document: &Document) -> bool {
    let overridden = |path: &str| {
        document
            .restart
            .iter()
            .any(|entry| entry.keys.iter().any(|key| key == path))
    };
    contract.schema.keys.iter().any(|key| {
        !key.reserved
            && (overridden(&key.path)
                || [Channel::Document, Channel::SecretsDir]
                    .into_iter()
                    .any(|channel| classify(contract, key, channel, false).needs_restart()))
    })
}

/// Whether the pod template carries any `checksum/` annotation.
///
/// Any rather than the exact name the generated set suggests: a chart that rolls on its whole
/// configuration carries its older full checksum, and is correct.
fn carries_checksum(workload: &Json) -> bool {
    let template = workload.get("spec").and_then(|spec| spec.get("template"));
    template
        .and_then(|template| template.get("metadata"))
        .and_then(|metadata| metadata.get("annotations"))
        .and_then(Json::as_object)
        .is_some_and(|annotations| annotations.keys().any(|name| name.starts_with("checksum/")))
}

/// Every live key of `contract` delivered through a mount the kubelet never updates.
fn frozen_mounts(
    manifests: &[Json],
    spec: &Json,
    container: &Json,
    container_name: &str,
    contract: &Contract,
    document: &Document,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let live = |channel: Channel| {
        contract
            .schema
            .keys
            .iter()
            .filter(move |key| classify(contract, key, channel, false) == Effective::Live)
    };

    // The document: a subPath mount of the object that holds it, or the object marked immutable.
    if live(Channel::Document).next().is_some()
        && let Some(object) = select(manifests, &document.source.kind, &document.source.selector)
            .into_iter()
            .next()
    {
        let object_name = object
            .get("metadata")
            .and_then(|metadata| metadata.get("name"))
            .and_then(Json::as_str)
            .unwrap_or("");
        if object.get("immutable").and_then(Json::as_bool) == Some(true) {
            findings.push(error(format!(
                "{} {object_name} holding {} is immutable, and `{container_name}`'s image applies \
                 keys in it without restarting; an immutable object never changes in place, so \
                 those keys are neither reloaded nor rolled",
                document.source.kind, document.name
            )));
        }
        let volumes = volumes_of(spec, &document.source.kind, object_name);
        for mount in mounts(container) {
            let mounted = mount.get("name").and_then(Json::as_str).unwrap_or("");
            if volumes.iter().any(|volume| volume == mounted) && mount.get("subPath").is_some() {
                findings.push(error(format!(
                    "`{container_name}` mounts {} with `subPath` at {}, and its image applies keys \
                     in it without restarting; the kubelet never updates a `subPath` mount, so \
                     those keys are neither reloaded nor rolled. Mount the directory instead.",
                    document.name,
                    mount.get("mountPath").and_then(Json::as_str).unwrap_or("?")
                )));
            }
        }
    }

    // The secrets directory: a live key's file mounted on its own with `subPath`.
    let live_files: Vec<&str> = live(Channel::SecretsDir)
        .flat_map(|key| {
            key.secrets_file
                .iter()
                .chain(&key.secrets_file_aliases)
                .map(String::as_str)
        })
        .collect();
    if let Some(directory) = secrets_dir(container, contract) {
        let directory = directory.trim_end_matches('/');
        for mount in mounts(container) {
            let path = mount.get("mountPath").and_then(Json::as_str).unwrap_or("");
            let Some(file) = path
                .strip_prefix(directory)
                .and_then(|rest| rest.strip_prefix('/'))
            else {
                continue;
            };
            if mount.get("subPath").is_some() && live_files.contains(&file) {
                findings.push(error(format!(
                    "`{container_name}` mounts the credential file {file} with `subPath`, and its \
                     image applies it without restarting; the kubelet never updates a `subPath` \
                     mount, so a rotation is neither reloaded nor rolled. Mount the secrets \
                     directory as a whole."
                )));
            }
        }
    }
    findings
}

/// The volumes of a pod spec that present the object `kind`/`name`, directly or projected.
fn volumes_of(spec: &Json, kind: &str, name: &str) -> Vec<String> {
    let (field, name_field) = match kind {
        "ConfigMap" => ("configMap", "name"),
        "Secret" => ("secret", "secretName"),
        _ => return Vec::new(),
    };
    spec.get("volumes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter(|volume| {
            let direct = volume
                .get(field)
                .and_then(|body| body.get(name_field))
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
                        .and_then(|body| body.get("name"))
                        .and_then(Json::as_str)
                        == Some(name)
                });
            direct || projected
        })
        .filter_map(|volume| volume.get("name").and_then(Json::as_str).map(str::to_owned))
        .collect()
}

/// A container's volume mounts.
fn mounts(container: &Json) -> impl Iterator<Item = &Json> {
    container
        .get("volumeMounts")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
}

/// The directory the container's loader reads key-named files from, when the render sets it.
fn secrets_dir<'a>(container: &'a Json, contract: &Contract) -> Option<&'a str> {
    let variable = contract
        .schema
        .loader
        .iter()
        .find(|var| var.role == crate::document::LoaderRole::SecretsDir)?;
    container
        .get("env")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .find(|entry| entry.get("name").and_then(Json::as_str) == Some(variable.env.as_str()))
        .and_then(|entry| entry.get("value"))
        .and_then(Json::as_str)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value as Json, json};

    use super::check_workload;
    use crate::document::Contract;
    use crate::gate::{DocumentFormat, DocumentSource};
    use crate::helm::declaration::{Binding, Consumer, Document};
    use crate::union::union_contracts;

    const DIGEST: &str = "sha256:aaaa";

    fn contract_json(reload: &Json) -> Json {
        let key = |path: &str, class: &str| {
            json!({
                "path": path, "env": format!("APP_{}", path.to_uppercase()), "env_file": null,
                "secrets_file": path, "docs": "", "ty": "String", "values": [],
                "text_form": "text", "aliases": [], "env_aliases": [], "env_file_aliases": [],
                "secrets_file_aliases": [], "default": null, "default_value": null, "note": null,
                "required": false, "secret": false, "reserved": false, "reload": class
            })
        };
        let mut schema = json!({
            "schema_version": 2,
            "dialect": { "prefix": "APP_", "nesting_separator": "__", "indirection_suffix": "_FILE" },
            "loader": [
                { "env": "APP_SECRETS_DIR", "role": "secrets_dir", "docs": "", "default": null }
            ],
            "keys": [key("token", "live"), key("level", "restart")]
        });
        if !reload.is_null() {
            schema["reload"] = reload.clone();
        }
        json!({
            "terrace_contract": 1,
            "producer": { "name": "t", "version": "0", "loader": "figment" },
            "app": { "name": "app" },
            "schema": schema,
            "json_schema": { "$schema": "http://json-schema.org/draft-07/schema#" },
            "external": { "env": [], "ignore": [], "unknown": "reject" }
        })
    }

    fn binding(reload: &Json) -> Binding {
        let raw = contract_json(reload);
        let union = union_contracts(&[("c.json".to_owned(), raw.clone())]).expect("a union");
        let contract = Contract::from_json(&raw.to_string()).expect("a contract");
        Binding {
            union: union.clone(),
            by_digest: BTreeMap::from([(DIGEST.to_owned(), union)]),
            contracts: BTreeMap::from([(DIGEST.to_owned(), contract)]),
        }
    }

    fn document() -> Document {
        Document {
            name: "server".to_owned(),
            source: DocumentSource {
                kind: "ConfigMap".to_owned(),
                selector: vec![("app".to_owned(), "x".to_owned())],
                key: "config.toml".to_owned(),
                format: DocumentFormat::Toml,
            },
            images: Vec::new(),
            consumers: Vec::new(),
            exempt: Vec::new(),
            restart: Vec::new(),
        }
    }

    fn consumer() -> Consumer {
        Consumer {
            kind: "Deployment".to_owned(),
            selector: Vec::new(),
            containers: vec!["app".to_owned()],
        }
    }

    fn manifests(annotations: &Json, mounts: &Json) -> Vec<Json> {
        vec![
            json!({
                "kind": "ConfigMap",
                "metadata": { "name": "app-config", "labels": { "app": "x" } },
                "data": { "config.toml": "" }
            }),
            json!({
                "kind": "Deployment",
                "metadata": { "name": "app" },
                "spec": { "template": {
                    "metadata": { "annotations": annotations },
                    "spec": {
                        "volumes": [
                            { "name": "config", "configMap": { "name": "app-config" } },
                            { "name": "secrets", "secret": { "secretName": "app-secrets" } }
                        ],
                        "containers": [{
                            "name": "app",
                            "image": format!("ghcr.io/x/app@{DIGEST}"),
                            "env": [{ "name": "APP_SECRETS_DIR", "value": "/secrets" }],
                            "volumeMounts": mounts
                        }]
                    }
                }}
            }),
        ]
    }

    fn rebuild() -> Json {
        json!({ "mode": "rebuild", "layers": ["document", "secrets_dir"] })
    }

    fn findings(reload: &Json, annotations: &Json, mounts: &Json) -> Vec<String> {
        let rendered = manifests(annotations, mounts);
        check_workload(
            &rendered,
            &rendered[1],
            &consumer(),
            &document(),
            &binding(reload),
        )
        .into_iter()
        .map(|finding| finding.message)
        .collect()
    }

    fn directory_mounts() -> Json {
        json!([
            { "name": "config", "mountPath": "/config" },
            { "name": "secrets", "mountPath": "/secrets" }
        ])
    }

    #[test]
    fn an_image_that_declares_nothing_is_not_this_gates_business() {
        assert!(findings(&Json::Null, &json!({}), &directory_mounts()).is_empty());
    }

    #[test]
    fn restart_keys_and_no_checksum_is_the_bug_this_exists_for() {
        let found = findings(&rebuild(), &json!({}), &directory_mounts());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("no `checksum/` annotation"), "{found:?}");

        let rolled = findings(
            &rebuild(),
            &json!({ "checksum/server-restart": "abc" }),
            &directory_mounts(),
        );
        assert!(rolled.is_empty(), "{rolled:?}");
    }

    #[test]
    fn a_live_key_behind_a_sub_path_mount_is_refused() {
        let found = findings(
            &rebuild(),
            &json!({ "checksum/server-restart": "abc" }),
            &json!([
                { "name": "config", "mountPath": "/config/config.toml", "subPath": "config.toml" },
                { "name": "secrets", "mountPath": "/secrets/token", "subPath": "token" }
            ]),
        );
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found[0].contains("mounts server with `subPath`"),
            "{found:?}"
        );
        assert!(found[1].contains("credential file token"), "{found:?}");
    }

    #[test]
    fn a_restart_key_behind_a_sub_path_mount_is_fine() {
        // `level` is restart: the checksum rolls the pod for it, so a frozen mount loses nothing.
        let found = findings(
            &rebuild(),
            &json!({ "checksum/server-restart": "abc" }),
            &json!([
                { "name": "config", "mountPath": "/config" },
                { "name": "secrets", "mountPath": "/secrets/level", "subPath": "level" }
            ]),
        );
        assert!(found.is_empty(), "{found:?}");
    }
}
