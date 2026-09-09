//! Gates 2 and 3 — one container's environment and file mounts, against its own image's contract.
//!
//! **Never the union.** A container runs exactly one image, so a variable set on it that only a
//! sibling image reads is precisely the defect gate 2 exists to catch, and checking it against the
//! merged contract of every image reading the shared document reintroduces what splitting the scopes
//! removed. Gate 1 is the opposite case: a file every binary reads.
//!
//! The container is scanned once into a [`ContainerView`], because the two gates are not
//! independent. The environment scan finds the secrets directory and every `_FILE` target, which is
//! what gate 3 needs to know; gate 3 finds which keys a mounted file supplies, which is what the
//! layer-collision check needs on top of what gate 2 found. Three passes over one container sharing
//! one reading of it, rather than three readings that could disagree.
//!
//! # The range step is asked for, not assumed
//!
//! [`crate::value::range`] returns [`Range::NotChecked`] for a document whose `producer.loader` this
//! build has no measured read table for, and this gate drops that answer rather than turning it into
//! a finding. Not because it does not matter — it is reported, at warning severity, by
//! [`crate::check`] once per document. It belongs there because it is a fact about the *document*
//! and not about any one value: reporting it per value would say the same sentence two hundred times
//! about one missing field.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::classify::{Kind, classify};
use crate::error::Error;
use crate::k8s::{Environment, environment_of, inside_a_mount, projected_file_names, volume_at};
use crate::report::{Finding, error, warning};
use crate::union::{Union, suggest};
use crate::value::{Range, form, range};

use super::{Relaxed, quoted};

/// The layer supplying a key from the environment.
const ENVIRONMENT_LAYER: &str = "the environment";
/// The layer supplying it from a key-named file in the secrets directory.
const SECRETS_LAYER: &str = "the secrets directory";

/// One container, read once: its environment classified, its loader variables resolved.
#[derive(Debug)]
pub struct ContainerView<'a> {
    /// The container's name, as the manifest gives it.
    pub name: String,
    /// The container itself.
    pub container: &'a Json,
    /// Its environment.
    pub environment: Environment,
    /// The path `<PREFIX>_SECRETS_DIR` — whatever the document calls it — points at.
    pub secrets_dir: Option<String>,
    /// Paths named by a `_FILE` variable, so gate 3 can tell a credential read by indirection from
    /// one merely lying in a volume nothing will open.
    pub indirect: BTreeMap<String, String>,
}

impl<'a> ContainerView<'a> {
    /// Read one container against the contract of the image it runs.
    pub fn read(container: &'a Json, union: &Union) -> Self {
        let environment = environment_of(container);
        let mut view = Self {
            name: container
                .get("name")
                .and_then(Json::as_str)
                .unwrap_or("?")
                .to_owned(),
            container,
            secrets_dir: None,
            indirect: BTreeMap::new(),
            environment,
        };

        for (variable, value) in view.environment.values.clone() {
            let decision = classify(union, &variable);
            match decision.kind {
                Kind::Loader
                    if decision.entry.and_then(|entry| entry.text("role"))
                        == Some("secrets_dir") =>
                {
                    view.secrets_dir = Some(value);
                }
                Kind::KeyEnvFile if !value.is_empty() && view.environment.visible(&variable) => {
                    view.indirect.insert(value, variable);
                }
                _ => {}
            }
        }
        view
    }
}

/// Which layer supplies each key, accumulated across gates 2 and 3.
///
/// A key supplied by two of the environment, the secrets directory and `_FILE` indirection is a boot
/// failure under a loader that refuses a shadowed key. Nothing else in a chart repository can see
/// it, because seeing it requires knowing that a variable and a file name are the same key.
#[derive(Debug, Default)]
struct Suppliers {
    layers: BTreeMap<String, Vec<String>>,
}

impl Suppliers {
    fn add(&mut self, path: &str, layer: String) {
        let held = self.layers.entry(path.to_owned()).or_default();
        if !held.contains(&layer) {
            held.push(layer);
        }
    }
}

/// `enableServiceLinks: false` is a precondition of these gates, not a preference.
///
/// Kubernetes injects `<SERVICE_NAME>_SERVICE_HOST`, `<SERVICE_NAME>_PORT` and five more per Service
/// in the namespace, and the service name is the *release* name — so for a release named after the
/// chart they land inside the loader's own prefix, and one of them can *supply* a key from the
/// environment layer, outranking the mounted file. That is a live misconfiguration, not merely a
/// validation nuisance.
///
/// It has to be checked directly rather than falling out of the classification, because the kubelet
/// injects these at pod admission and a template render does not: a rendered manifest never carries
/// one, so no amount of classification would find it. This can only require the switch that stops
/// them existing.
///
/// An image cannot declare them away either — it cannot know the release names it will be deployed
/// under, so no declaration written at build time is right for every case. It belongs to whatever
/// renders the deployment, which does know.
pub struct ServiceLinks;

impl ServiceLinks {
    /// Whether the pod switched the injection off.
    pub fn check(spec: &Json, union: &Union) -> Vec<Finding> {
        if spec.get("enableServiceLinks") == Some(&Json::Bool(false)) {
            return Vec::new();
        }
        vec![error(format!(
            "pod: the pod spec does not set `enableServiceLinks: false`. Kubernetes injects seven \
             variables per Service in the namespace, named after the release, so a release named \
             for this chart puts them inside the {} namespace, where one can supply a key from the \
             environment layer and outrank the mounted file. The kubelet injects them at admission \
             rather than `helm template` doing it, so this gate cannot see them and can only \
             require the switch.",
            union.prefix()
        ))]
    }
}

/// Run gates 2 and 3 over one container, against the contract of the image it runs.
///
/// # Errors
/// [`Error::Invalid`] when the contract carries a `text_form` or a constraint keyword this build
/// does not implement, which is a document this consumer cannot read rather than a tree it can
/// judge.
pub fn check_container(
    manifests: &[Json],
    spec: &Json,
    container: &Json,
    union: &Union,
    relaxed: Relaxed,
) -> Result<Vec<Finding>, Error> {
    let view = ContainerView::read(container, union);
    let mut suppliers = Suppliers::default();

    let mut findings = environment_gate(&view, union, &mut suppliers, relaxed)?;
    findings.extend(file_gate(
        manifests,
        spec,
        &view,
        union,
        &mut suppliers,
        relaxed,
    )?);
    findings.extend(collision_gate(&view, &suppliers, relaxed));
    Ok(findings)
}

/// Gate 2 — every variable on one container, classified and checked.
///
/// The classification order is normative and first match wins. Every variable that names a key is
/// then checked twice: its text against `text_constraint`, then the value that text reads as against
/// `constraint`. Both, or the document's bounds are decorative and a render that passes every gate
/// still fails at boot.
fn environment_gate(
    view: &ContainerView<'_>,
    union: &Union,
    suppliers: &mut Suppliers,
    relaxed: Relaxed,
) -> Result<Vec<Finding>, Error> {
    let mut findings = Vec::new();

    for (variable, value) in view.environment.sorted() {
        let decision = classify(union, variable);
        match decision.kind {
            Kind::KeyEnv => {
                let entry = decision.entry.expect("a matched key carries its entry");
                suppliers.add(&entry.name, ENVIRONMENT_LAYER.to_owned());
                if !relaxed.env && view.environment.visible(variable) {
                    findings.extend(check_value(view, union, variable, value, entry)?);
                }
            }
            Kind::KeyEnvFile => {
                let entry = decision.entry.expect("a matched key carries its entry");
                suppliers.add(&entry.name, format!("the file named by {variable}"));
            }
            Kind::Prefixed if !relaxed.env => {
                // A variable can address a leaf of a dynamic map the same way a file can, and for
                // the same reason the contract cannot name it. Checked here rather than inside the
                // classification, whose step order is normative — this is an implementation
                // necessity found against a real chart, not a new step.
                if let Some(container) = union.container_of("env", variable) {
                    suppliers.add(&container.name, ENVIRONMENT_LAYER.to_owned());
                } else {
                    findings.push(error(format!(
                        "env: {variable} set by container {} matches no key in the contract{}",
                        quoted(&view.name),
                        suggest(variable, union.spellings("env"))
                    )));
                }
            }
            Kind::External => {
                if !relaxed.env && view.environment.visible(variable) {
                    let entry = decision
                        .entry
                        .expect("a matched variable carries its entry");
                    findings.extend(check_value(view, union, variable, value, entry)?);
                }
            }
            Kind::Unknown if !relaxed.env => {
                let message = format!(
                    "env: {variable} is set by container {} and the contract accounts for it \
                     nowhere: it is not a key, not a declared external variable, and matches no \
                     ignore pattern",
                    quoted(&view.name)
                );
                match union.unknown.as_str() {
                    "reject" => findings.push(error(message)),
                    "warn" => findings.push(warning(message)),
                    _ => {}
                }
            }
            Kind::Loader | Kind::Ignored | Kind::Prefixed | Kind::Unknown => {}
        }
    }

    Ok(findings)
}

/// The two-step check, and the range step only when the form step passed.
///
/// One value produces one line: "not an integer" followed by "not below 65535" says nothing the
/// first did not, and the second would be reporting on a read that never happened.
fn check_value(
    view: &ContainerView<'_>,
    union: &Union,
    variable: &str,
    value: &str,
    entry: &crate::union::Merged,
) -> Result<Vec<Finding>, Error> {
    if let Some(failure) = form(entry.entry(), value)? {
        return Ok(vec![error(format!(
            "env: {variable} on container {}: {failure}",
            quoted(&view.name)
        ))]);
    }
    Ok(match range(entry.entry(), &union.loader_name, value)? {
        Range::Wrong(failure) => vec![error(format!(
            "env: {variable} on container {}: {failure}",
            quoted(&view.name)
        ))],
        // Reported once per document by the caller, because it is one fact about the document
        // rather than one about each of its values.
        Range::Ok | Range::NotChecked(_) => Vec::new(),
    })
}

/// Gate 3 — the file spellings, over every volume rather than only the secrets directory.
///
/// Keying this off the secrets-directory variable alone leaves the worse half of the defect
/// invisible: a chart that mounts key-named credential files and never sets the variable has produced
/// a pod where the files exist, the loader never looks at them, and every credential falls back to a
/// default. Nothing renders wrong and nothing fails to start. So every mount is inspected, and a file
/// whose name spells a key is a finding wherever it turns up — unless a `_FILE` variable names it,
/// which is the other legitimate way for a file to be read.
fn file_gate(
    manifests: &[Json],
    spec: &Json,
    view: &ContainerView<'_>,
    union: &Union,
    suppliers: &mut Suppliers,
    relaxed: Relaxed,
) -> Result<Vec<Finding>, Error> {
    if relaxed.files {
        return Ok(Vec::new());
    }
    let mut findings = loader_paths(view, union);
    findings.extend(indirection(view, union)?);
    findings.extend(mounts(manifests, spec, view, union, suppliers)?);
    Ok(findings)
}

/// A loader variable naming a path nothing mounts is a boot failure, not an empty layer.
fn loader_paths(view: &ContainerView<'_>, union: &Union) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (variable, value) in view.environment.sorted() {
        let decision = classify(union, variable);
        if decision.kind != Kind::Loader || value.is_empty() || !view.environment.visible(variable)
        {
            continue;
        }
        let role = decision.entry.and_then(|entry| entry.text("role"));
        if !matches!(role, Some("config" | "secrets_dir")) {
            continue;
        }
        if !inside_a_mount(view.container, value) {
            findings.push(error(format!(
                "files: {variable} on container {} points at {}, which is not inside any volume \
                 the container mounts; the loader fails its boot on a configured path it cannot \
                 read",
                quoted(&view.name),
                quoted(value)
            )));
        }
    }
    findings
}

/// Every `_FILE` variable: whether the key it names can be file-supplied, and whether the path is
/// mounted.
fn indirection(view: &ContainerView<'_>, union: &Union) -> Result<Vec<Finding>, Error> {
    let mut findings = Vec::new();
    for (variable, value) in view.environment.sorted() {
        let decision = classify(union, variable);
        if decision.kind != Kind::KeyEnvFile {
            continue;
        }
        let key = decision.entry.expect("a matched key carries its entry");
        findings.extend(file_typed(
            key,
            &format!(
                "{variable} on container {} names a file",
                quoted(&view.name)
            ),
        )?);
        if !view.environment.visible(variable) || value.is_empty() {
            continue;
        }
        if !inside_a_mount(view.container, value) {
            findings.push(error(format!(
                "files: {variable} on container {} points at {}, which is not inside any volume \
                 the container mounts",
                quoted(&view.name),
                quoted(value)
            )));
        }
    }
    Ok(findings)
}

/// Every file name every mount presents, against the key spellings the contract publishes.
fn mounts(
    manifests: &[Json],
    spec: &Json,
    view: &ContainerView<'_>,
    union: &Union,
    suppliers: &mut Suppliers,
) -> Result<Vec<Finding>, Error> {
    let mut findings = Vec::new();
    let spellings = union.spellings("secrets_file");
    let configured = view
        .secrets_dir
        .as_deref()
        .map(|path| path.trim_end_matches('/'));

    for mount in view
        .container
        .get("volumeMounts")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let Some(volume) = mount
            .get("name")
            .and_then(Json::as_str)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        let path = mount
            .get("mountPath")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .trim_end_matches('/');
        let is_secrets_dir = configured == Some(path);

        for file_name in projected_file_names(manifests, spec, volume) {
            let key = union.key_by("secrets_file", &file_name);

            if is_secrets_dir {
                let Some(key) = key else {
                    // Before calling it a misspelling: it may be addressing a leaf inside a dynamic
                    // map, whose sub-keys the deployment chooses and no build-time contract can
                    // enumerate.
                    if let Some(container) = union.container_of("secrets_file", &file_name) {
                        suppliers.add(&container.name, SECRETS_LAYER.to_owned());
                        continue;
                    }
                    findings.push(error(format!(
                        "files: the secrets directory mounts {}, which spells no key in the \
                         contract{}",
                        quoted(&file_name),
                        suggest(&file_name, spellings.iter().copied())
                    )));
                    continue;
                };
                suppliers.add(&key.name, SECRETS_LAYER.to_owned());
                findings.extend(file_typed(
                    key,
                    &format!("the secrets directory mounts {}", quoted(&file_name)),
                )?);
                continue;
            }

            let Some(key) = key else { continue };
            if view.indirect.contains_key(&format!("{path}/{file_name}")) {
                continue;
            }
            findings.push(error(format!(
                "files: {path}/{file_name} spells the key {}, but nothing reads it: {}",
                quoted(&key.name),
                match configured {
                    Some(configured) => format!(
                        "the secrets directory is {} and no `_FILE` variable names it",
                        quoted(configured)
                    ),
                    None => "no secrets directory is configured on this container and no `_FILE` \
                             variable names it"
                        .to_owned(),
                }
            )));
        }
    }

    if let Some(configured) = configured
        && volume_at(view.container, configured).is_none()
        && inside_a_mount(view.container, configured)
    {
        findings.push(warning(format!(
            "files: the secrets directory {} on container {} is inside a volume rather than \
                 the mount point itself, so the file names it will present cannot be read from the \
                 manifest and gate 3 checked none of them",
            quoted(configured),
            quoted(&view.name)
        )));
    }

    Ok(findings)
}

/// A file can only supply a key whose `text_form` is `text`.
fn file_typed(key: &crate::union::Merged, what: &str) -> Result<Vec<Finding>, Error> {
    if key.entry().file_supplyable()? {
        return Ok(Vec::new());
    }
    // The producer's own name for the type, printed and never matched on: it is token text in the
    // producer's language, and `java.time.Duration` is as legitimate a spelling here as `u64`.
    let named = key.text("ty").map_or("None".to_owned(), quoted);
    Ok(vec![error(format!(
        "files: {what} for the key {}, whose text_form is {} (type {named}): a file's contents \
         arrive as a string with no parse and the loader does not coerce one into a number, a \
         boolean or a TOML literal, so this key cannot be supplied by a file at all",
        quoted(&key.name),
        key.text("text_form").map_or("None".to_owned(), quoted)
    ))])
}

/// The collision the loader refuses at boot, once both gates have said who supplies what.
fn collision_gate(
    view: &ContainerView<'_>,
    suppliers: &Suppliers,
    relaxed: Relaxed,
) -> Vec<Finding> {
    if relaxed.env {
        return Vec::new();
    }
    let mut findings = Vec::new();
    for (path, layers) in &suppliers.layers {
        if layers.len() > 1 {
            let mut sorted = layers.clone();
            sorted.sort();
            findings.push(error(format!(
                "env: the key {} is supplied by {} on container {}; the loader refuses a key \
                 supplied by two of the environment, the secrets directory and `_FILE` \
                 indirection, and fails its boot naming the key",
                quoted(path),
                sorted.join(" and "),
                quoted(&view.name)
            )));
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use serde_json::{Value as Json, json};

    use super::{Relaxed, ServiceLinks, check_container};
    use crate::report::Level;
    use crate::union::{Union, union_contracts};

    fn union(keys: &Json, loader: &Json, producer_loader: &str) -> Union {
        let mut contract = json!({
            "terrace_contract": 1,
            "producer": {"name": "x", "version": "1", "loader": producer_loader},
            "app": {"name": "x"},
            "schema": {
                "schema_version": 2,
                "dialect": {"prefix": "P_", "nesting_separator": "__", "indirection_suffix": "_FILE"},
                "loader": loader.clone(), "keys": keys.clone(),
            },
            "json_schema": {},
            "external": {"env": [], "ignore": [], "unknown": "reject"},
        });
        if producer_loader.is_empty() {
            contract
                .as_object_mut()
                .expect("an object")
                .remove("producer");
        }
        union_contracts(&[("a".to_owned(), contract)]).expect("one contract merges")
    }

    fn check(container: &Json, union: &Union) -> Vec<crate::report::Finding> {
        check_container(&[], &json!({}), container, union, Relaxed::default())
            .expect("the contract is readable")
    }

    #[test]
    fn a_value_outside_a_bound_is_found_when_the_loader_is_known() {
        let merged = union(
            &json!([{"path": "server.port", "env": "P_SERVER__PORT", "text_form": "integer",
                    "constraint": {"type": "integer", "maximum": 65535}}]),
            &json!([]),
            "figment",
        );
        let container =
            json!({"name": "app", "env": [{"name": "P_SERVER__PORT", "value": "99999"}]});
        let findings = check(&container, &merged);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].message.contains("above the maximum 65535"),
            "{findings:?}"
        );
    }

    #[test]
    fn the_same_value_is_not_a_finding_when_the_loader_is_not() {
        // The expensive direction: a bound measured against another loader's reads would stop a
        // deployment that was correct. The skip is reported once per document by the caller.
        let merged = union(
            &json!([{"path": "server.port", "env": "P_SERVER__PORT", "text_form": "integer",
                    "constraint": {"type": "integer", "maximum": 65535}}]),
            &json!([]),
            "spring-boot",
        );
        let container =
            json!({"name": "app", "env": [{"name": "P_SERVER__PORT", "value": "99999"}]});
        assert_eq!(check(&container, &merged), Vec::new());
    }

    #[test]
    fn the_form_step_still_runs_for_an_unknown_loader() {
        // `text_constraint` is what the producer published about its own reads, so holding text to
        // it is repeating the producer's statement rather than making one.
        let merged = union(
            &json!([{"path": "a", "env": "P_A", "text_form": "integer",
                    "text_constraint": {"type": "string", "pattern": "^[0-9]+$"}}]),
            &json!([]),
            "spring-boot",
        );
        let container = json!({"name": "app", "env": [{"name": "P_A", "value": "http"}]});
        let findings = check(&container, &merged);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].message.contains("does not match"),
            "{findings:?}"
        );
    }

    #[test]
    fn a_variable_inside_the_namespace_that_spells_nothing_is_named_with_its_near_miss() {
        let merged = union(
            &json!([{"path": "server.port", "env": "P_SERVER__PORT", "text_form": "integer"}]),
            &json!([]),
            "figment",
        );
        let container = json!({"name": "app", "env": [{"name": "P_SERVER__PORTS", "value": "1"}]});
        let findings = check(&container, &merged);
        assert_eq!(
            findings[0].message,
            "env: P_SERVER__PORTS set by container 'app' matches no key in the contract (did you \
             mean P_SERVER__PORT?)"
        );
    }

    #[test]
    fn a_variable_no_rule_accounts_for_follows_the_documents_own_policy() {
        let merged = union(&json!([]), &json!([]), "figment");
        let container = json!({"name": "app", "env": [{"name": "HOME", "value": "/root"}]});
        let findings = check(&container, &merged);
        assert_eq!(findings[0].level, Level::Error);
        assert!(
            findings[0].message.contains("accounts for it nowhere"),
            "{findings:?}"
        );
    }

    #[test]
    fn a_value_from_is_classified_and_its_value_is_not_checked() {
        let merged = union(
            &json!([{"path": "a", "env": "P_A", "text_form": "integer"}]),
            &json!([]),
            "figment",
        );
        let container = json!({"name": "app", "env": [
            {"name": "P_A", "valueFrom": {"secretKeyRef": {"name": "s", "key": "k"}}}
        ]});
        assert_eq!(check(&container, &merged), Vec::new());
    }

    #[test]
    fn a_key_supplied_by_two_layers_is_the_boot_failure_nothing_else_can_see() {
        let merged = union(
            &json!([{"path": "a", "env": "P_A", "env_file": "P_A_FILE", "text_form": "text"}]),
            &json!([]),
            "figment",
        );
        let container = json!({"name": "app", "env": [
            {"name": "P_A", "value": "x"},
            {"name": "P_A_FILE", "value": "/secrets/a"},
        ], "volumeMounts": [{"name": "v", "mountPath": "/secrets"}]});
        let findings = check(&container, &merged);
        let collision = findings
            .iter()
            .find(|finding| finding.message.contains("supplied by"))
            .expect("the collision is found");
        assert_eq!(
            collision.message,
            "env: the key 'a' is supplied by the environment and the file named by P_A_FILE on \
             container 'app'; the loader refuses a key supplied by two of the environment, the \
             secrets directory and `_FILE` indirection, and fails its boot naming the key"
        );
    }

    #[test]
    fn a_file_cannot_supply_a_key_that_is_not_text_and_the_type_is_only_printed() {
        let merged = union(
            &json!([{"path": "ttl", "env_file": "P_TTL_FILE", "text_form": "integer",
                    "ty": "java.time.Duration"}]),
            &json!([]),
            "figment",
        );
        let container = json!({"name": "app", "env": [
            {"name": "P_TTL_FILE", "value": "/secrets/ttl"}
        ], "volumeMounts": [{"name": "v", "mountPath": "/secrets"}]});
        let findings = check(&container, &merged);
        assert!(
            findings[0].message.contains("(type 'java.time.Duration')"),
            "{findings:?}"
        );
    }

    #[test]
    fn a_loader_path_nothing_mounts_is_a_boot_failure() {
        let merged = union(
            &json!([]),
            &json!([{"env": "P_CONFIG", "role": "config"}]),
            "figment",
        );
        let container =
            json!({"name": "app", "env": [{"name": "P_CONFIG", "value": "/etc/app.toml"}]});
        let findings = check(&container, &merged);
        assert!(
            findings[0].message.contains("not inside any volume"),
            "{findings:?}"
        );
    }

    #[test]
    fn a_pod_that_does_not_switch_service_links_off_is_a_finding() {
        let merged = union(&json!([]), &json!([]), "figment");
        assert_eq!(
            ServiceLinks::check(&json!({"enableServiceLinks": false}), &merged),
            Vec::new()
        );
        assert_eq!(ServiceLinks::check(&json!({}), &merged).len(), 1);
    }

    #[test]
    fn a_relaxed_env_gate_checks_no_variable_and_reports_no_collision() {
        let merged = union(
            &json!([{"path": "a", "env": "P_A", "env_file": "P_A_FILE", "text_form": "text"}]),
            &json!([]),
            "figment",
        );
        let container = json!({"name": "app", "env": [
            {"name": "P_A", "value": "x"},
            {"name": "P_A_FILE", "value": "/secrets/a"},
            {"name": "HOME", "value": "/root"},
        ], "volumeMounts": [{"name": "v", "mountPath": "/secrets"}]});
        let relaxed = Relaxed {
            env: true,
            ..Relaxed::default()
        };
        assert_eq!(
            check_container(&[], &json!({}), &container, &merged, relaxed)
                .expect("the contract is readable"),
            Vec::new()
        );
    }
}
