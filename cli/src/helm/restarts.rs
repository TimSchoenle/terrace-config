//! `templates/_config-reload.tpl`: the part of each document a chart rolls its pods for.
//!
//! A chart rolls a workload by changing its pod template, and the usual way is a `checksum/`
//! annotation over the configuration it mounts. Hashing *all* of it rolls the pods on every change
//! and throws away an image's in-place reload; hashing *none* of it leaves every value the process
//! reads only at start changed on disk and never applied. The contract says which keys are which
//! (see [`crate::reload`]), so the chart hashes exactly the keys that need a restart — and this
//! module writes the list it hashes, from the contracts it vendors.
//!
//! # Generated rather than read at render time
//!
//! The vendored contracts are deliberately left out of a packaged chart, and they run to hundreds of
//! kilobytes: parsing them in a template on every install would reverse the first decision and pay
//! for it on every render. So the list is generated into a template file the chart ships, and
//! `--check` fails when it and the contracts disagree, which is how the README reference stays
//! honest too.
//!
//! # One set per document
//!
//! Which image a container runs is resolved by digest from a *render*, and this runs without one.
//! So a key is in a document's set when any image that reads the document calls it `restart` —
//! conservative for a document several images read, exact for one image's. An image that declares
//! no rebuild at all, or does not watch the layer, puts the whole of that layer in the set.
//!
//! # What the chart does with it
//!
//! Each `define` yields YAML a template reads with `include … | fromYaml`:
//!
//! ```text
//! {{- define "portfolio.terrace.reload.server" -}}
//! document:
//!   all: false
//!   paths:
//!     - ["log_filter"]
//! secrets:
//!   all: false
//!   files:
//!     - "storage__key"
//! {{- end -}}
//! ```
//!
//! `document.paths` are key paths as segment lists, aliases included, to dig out of the document
//! the chart builds before serialising it; `secrets.files` are the names in the chart's own
//! Secret. `all: true` means hash that half entirely. Content a chart cannot attribute to a key — a
//! verbatim fragment appended to the document — belongs in the digest whatever this says.
//!
//! `_FILE` indirection targets are not attributed: the file a variable names can live in any
//! object, and a chart delivering configuration that way hashes that object in full.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::document::{Contract, ReloadLayer};
use crate::error::Error;
use crate::reload::{image_class, rebuilds};

use super::declaration::{Declaration, Document, vendored_for};
use super::secrets::contracted_charts;

/// Where the generated file lives, relative to the chart.
pub const TEMPLATE: &str = "templates/_config-reload.tpl";

/// The keys of one document a chart rolls its pods for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartSet {
    /// The document, as the declaration names it.
    pub document: String,
    /// Whether the whole rendered document is restart content.
    pub document_all: bool,
    /// The restart-class key paths, aliases included, each as its segments. Sorted.
    pub paths: Vec<Vec<String>>,
    /// Whether every file in the chart's Secret is restart content.
    pub secrets_all: bool,
    /// The restart-class secrets-directory file names, aliases included. Sorted.
    pub files: Vec<String>,
}

/// What one pass over the chart tree found.
#[derive(Debug, Default)]
pub struct Written {
    /// Everything that stopped a file being written or made it wrong.
    pub problems: Vec<String>,
    /// How many files were written.
    pub touched: usize,
}

/// Write, or compare, every contracted chart's restart sets.
///
/// A chart none of whose images rebuilds, and which carries no generated file, is skipped: it rolls
/// on its full configuration as it always has, and nothing here has anything to add. A chart that
/// carries the file keeps it current whatever its images declare.
///
/// # Errors
/// [`Error::Invalid`] when a declaration or a contract cannot be read at all, [`Error::Io`] when the
/// tree cannot be walked or a file cannot be written. A chart whose *set* is wrong is a problem on
/// the result rather than an error, so the run still answers for every other chart.
pub fn walk(charts: &Path, check: bool) -> Result<Written, Error> {
    let mut written = Written::default();
    for (chart_dir, declaration) in contracted_charts(charts)? {
        one_chart(&chart_dir, &declaration, check, &mut written)?;
    }
    Ok(written)
}

/// One chart's file.
fn one_chart(
    chart_dir: &Path,
    declaration: &Declaration,
    check: bool,
    written: &mut Written,
) -> Result<(), Error> {
    let target = chart_dir.join(TEMPLATE);
    let mut sets = Vec::new();
    let mut any_rebuilds = false;

    for document in &declaration.documents {
        let contracts = typed_contracts(chart_dir, document)?;
        any_rebuilds = any_rebuilds || contracts.iter().any(|(_, contract)| rebuilds(contract));
        match restart_set(document, &contracts) {
            Ok(set) => sets.push(set),
            Err(problems) => {
                for problem in problems {
                    written.problems.push(format!(
                        "{}: {}: {problem}",
                        declaration.chart, document.name
                    ));
                }
                return Ok(());
            }
        }
    }

    let exists = target.is_file();
    if !any_rebuilds && !exists {
        return Ok(());
    }

    let rendered = render(&declaration.chart, &sets);
    let current = if exists {
        std::fs::read_to_string(&target).map_err(|e| Error::io(target.display(), e))?
    } else {
        String::new()
    };
    if current == rendered {
        return Ok(());
    }
    if check {
        written.problems.push(if exists {
            format!(
                "{}: the restart sets are not what the contracts describe; regenerate them\n{}",
                target.display(),
                super::readme::difference(&current, &rendered, &target.display().to_string())
            )
        } else {
            format!(
                "{}: an image this chart pins applies changes without restarting, and the chart \
                 carries no restart sets to roll its pods by; generate them",
                target.display()
            )
        });
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display(), e))?;
    }
    std::fs::write(&target, rendered).map_err(|e| Error::io(target.display(), e))?;
    written.touched += 1;
    Ok(())
}

/// Every contract a document's images publish, typed, labelled as the chart vendors them.
///
/// # Errors
/// [`Error::Invalid`] when a vendored contract cannot be read as one.
pub fn typed_contracts(
    chart_dir: &Path,
    document: &Document,
) -> Result<Vec<(String, Contract)>, Error> {
    vendored_for(chart_dir, document)?
        .into_iter()
        .map(|item| {
            let contract = Contract::from_json(&item.vendored.contract.to_string())
                .map_err(|e| Error::Invalid(format!("{}: {e}", item.label)))?;
            Ok((item.label, contract))
        })
        .collect()
}

/// One document's restart set, from the contracts of the images that read it.
///
/// # Errors
/// Every `restart:` override in the declaration that names a key none of the contracts declares —
/// an entry that has outlived the key it described, and would otherwise sit in the file promising
/// a rollout for a setting nothing reads.
pub fn restart_set(
    document: &Document,
    contracts: &[(String, Contract)],
) -> Result<RestartSet, Vec<String>> {
    let overridden: BTreeSet<&str> = document
        .restart
        .iter()
        .flat_map(|entry| entry.keys.iter().map(String::as_str))
        .collect();

    let declared: BTreeSet<&str> = contracts
        .iter()
        .flat_map(|(_, contract)| contract.schema.keys.iter().map(|key| key.path.as_str()))
        .collect();
    let stale: Vec<String> = overridden
        .iter()
        .filter(|path| !declared.contains(*path))
        .map(|path| {
            format!(
                "`restart` names `{path}`, which no contract this document reads declares; drop \
                 the entry, or correct the path"
            )
        })
        .collect();
    if !stale.is_empty() {
        return Err(stale);
    }

    let watches = |layer: ReloadLayer| {
        contracts.iter().all(|(_, contract)| {
            rebuilds(contract)
                && contract
                    .schema
                    .reload
                    .as_ref()
                    .is_some_and(|support| support.layers.contains(&layer))
        })
    };
    let document_all = !watches(ReloadLayer::Document);
    let secrets_all = !watches(ReloadLayer::SecretsDir);

    let mut paths: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut files: BTreeSet<String> = BTreeSet::new();
    for (_, contract) in contracts {
        for key in &contract.schema.keys {
            // Read from the environment before the layers exist, so it is in neither half.
            if key.reserved {
                continue;
            }
            let restart =
                image_class(contract, key).is_err() || overridden.contains(key.path.as_str());
            if !restart {
                continue;
            }
            for path in std::iter::once(&key.path).chain(&key.aliases) {
                paths.insert(path.split('.').map(str::to_owned).collect());
            }
            files.extend(key.secrets_file.iter().cloned());
            files.extend(key.secrets_file_aliases.iter().cloned());
        }
    }

    Ok(RestartSet {
        document: document.name.clone(),
        document_all,
        paths: if document_all {
            Vec::new()
        } else {
            paths.into_iter().collect()
        },
        secrets_all,
        files: if secrets_all {
            Vec::new()
        } else {
            files.into_iter().collect()
        },
    })
}

/// The whole file for one chart.
#[must_use]
pub fn render(chart: &str, sets: &[RestartSet]) -> String {
    let mut ordered: BTreeMap<&str, &RestartSet> = BTreeMap::new();
    for set in sets {
        ordered.insert(set.document.as_str(), set);
    }

    let mut out = String::from(HEADER);
    for (document, set) in ordered {
        let name = quoted(&format!("{chart}.terrace.reload.{document}"));
        let _ = write!(out, "\n{{{{- define {name} -}}}}\ndocument:\n");
        let _ = writeln!(out, "  all: {}", set.document_all);
        list(
            &mut out,
            "paths",
            set.paths.iter().map(|segments| {
                let segments: Vec<String> =
                    segments.iter().map(|segment| quoted(segment)).collect();
                format!("[{}]", segments.join(", "))
            }),
        );
        let _ = writeln!(out, "secrets:\n  all: {}", set.secrets_all);
        list(&mut out, "files", set.files.iter().map(|file| quoted(file)));
        out.push_str("{{- end -}}\n");
    }
    out
}

/// The comment every generated file opens with.
const HEADER: &str = "{{- /*
Generated by `terrace-contract reload` from the contracts this chart vendors. Do not
edit: `terrace-contract reload --check` fails when this file and the contracts disagree.

One define per configuration document, naming the parts of it only a process restart
applies. A pod annotation hashing exactly those parts rolls the workload for a change
that needs it, and leaves every other change to the image's in-place reload. `all: true`
means the whole of that half is restart content.
*/}}
";

/// One YAML list under a two-space indent, in flow form when it is empty.
fn list(out: &mut String, name: &str, items: impl Iterator<Item = String>) {
    let items: Vec<String> = items.collect();
    if items.is_empty() {
        let _ = writeln!(out, "  {name}: []");
        return;
    }
    let _ = writeln!(out, "  {name}:");
    for item in items {
        let _ = writeln!(out, "    - {item}");
    }
}

/// A YAML double-quoted string that is also inert to the template engine.
///
/// JSON escaping is valid YAML double-quoted escaping, and braces are escaped on top of it: a key
/// path is whatever a producer published, and `{{` in one would otherwise open a template action in
/// the middle of the file.
fn quoted(text: &str) -> String {
    serde_json::to_string(text)
        .unwrap_or_default()
        .replace('{', "\\u007b")
        .replace('}', "\\u007d")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{RestartSet, render, restart_set};
    use crate::document::Contract;
    use crate::gate::{DocumentFormat, DocumentSource};
    use crate::helm::declaration::{Document, Restarted};

    fn key(path: &str, class: Option<&str>) -> serde_json::Value {
        let file = path.replace('.', "__");
        let mut key = json!({
            "path": path, "env": format!("APP_{}", file.to_uppercase()),
            "env_file": null, "secrets_file": file, "docs": "", "ty": "String", "values": [],
            "text_form": "text", "aliases": [], "env_aliases": [], "env_file_aliases": [],
            "secrets_file_aliases": [], "default": null, "default_value": null, "note": null,
            "required": false, "secret": false, "reserved": false
        });
        if let Some(class) = class {
            key["reload"] = json!(class);
        }
        key
    }

    fn contract(reload: &serde_json::Value, keys: &[serde_json::Value]) -> Contract {
        let mut schema = json!({
            "schema_version": 2,
            "dialect": { "prefix": "APP_", "nesting_separator": "__", "indirection_suffix": "_FILE" },
            "loader": [],
            "keys": keys
        });
        if !reload.is_null() {
            schema["reload"] = reload.clone();
        }
        serde_json::from_value(json!({
            "terrace_contract": 1,
            "producer": { "name": "t", "version": "0", "loader": "figment" },
            "app": { "name": "app" },
            "schema": schema,
            "json_schema": {},
            "external": { "env": [], "ignore": [], "unknown": "reject" }
        }))
        .expect("a contract")
    }

    fn document(restart: Vec<Restarted>) -> Document {
        Document {
            name: "server".to_owned(),
            source: DocumentSource {
                kind: "ConfigMap".to_owned(),
                selector: Vec::new(),
                key: "config.toml".to_owned(),
                format: DocumentFormat::Toml,
            },
            images: Vec::new(),
            consumers: Vec::new(),
            exempt: Vec::new(),
            restart,
        }
    }

    fn rebuild() -> serde_json::Value {
        json!({ "mode": "rebuild", "layers": ["document", "secrets_dir", "env_file"] })
    }

    fn set(contracts: &[Contract], restart: Vec<Restarted>) -> RestartSet {
        let labelled: Vec<(String, Contract)> = contracts
            .iter()
            .map(|contract| ("c.json".to_owned(), contract.clone()))
            .collect();
        restart_set(&document(restart), &labelled).expect("a set")
    }

    #[test]
    fn only_the_restart_keys_are_listed() {
        let found = set(
            &[contract(
                &rebuild(),
                &[key("ttl", Some("live")), key("log.level", Some("restart"))],
            )],
            Vec::new(),
        );
        assert!(!found.document_all);
        assert_eq!(found.paths, [vec!["log".to_owned(), "level".to_owned()]]);
        assert_eq!(found.files, ["log__level"]);
    }

    #[test]
    fn an_image_that_does_not_rebuild_makes_everything_restart_content() {
        let found = set(
            &[
                contract(&rebuild(), &[key("ttl", Some("live"))]),
                contract(&serde_json::Value::Null, &[key("ttl", None)]),
            ],
            Vec::new(),
        );
        assert!(found.document_all && found.secrets_all);
    }

    #[test]
    fn a_key_one_image_calls_restart_is_restart_for_the_document() {
        let found = set(
            &[
                contract(&rebuild(), &[key("ttl", Some("live"))]),
                contract(&rebuild(), &[key("ttl", Some("restart"))]),
            ],
            Vec::new(),
        );
        assert_eq!(found.paths, [vec!["ttl".to_owned()]]);
    }

    #[test]
    fn a_chart_can_promote_a_key_and_a_stale_promotion_is_refused() {
        let live = contract(&rebuild(), &[key("ttl", Some("live"))]);
        let promoted = set(
            std::slice::from_ref(&live),
            vec![Restarted {
                keys: vec!["ttl".to_owned()],
                reason: "canaried".to_owned(),
            }],
        );
        assert_eq!(promoted.paths, [vec!["ttl".to_owned()]]);

        let stale = restart_set(
            &document(vec![Restarted {
                keys: vec!["gone".to_owned()],
                reason: "was here once".to_owned(),
            }]),
            &[("c.json".to_owned(), live)],
        )
        .expect_err("a key nothing declares");
        assert!(stale[0].contains("`gone`"), "{stale:?}");
    }

    #[test]
    fn the_file_is_stable_and_inert_to_the_template_engine() {
        let sets = [RestartSet {
            document: "server".to_owned(),
            document_all: false,
            paths: vec![vec!["odd{{".to_owned(), "key".to_owned()]],
            secrets_all: true,
            files: Vec::new(),
        }];
        let rendered = render("chart", &sets);
        assert!(rendered.contains("{{- define \"chart.terrace.reload.server\" -}}"));
        assert!(
            rendered.contains(r#"- ["odd\u007b\u007b", "key"]"#),
            "{rendered}"
        );
        assert!(rendered.contains("  files: []\n"), "{rendered}");
        assert_eq!(rendered, render("chart", &sets));
    }
}
