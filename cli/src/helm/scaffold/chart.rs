//! The whole configuration files a new chart starts with, composed from the planner's rules.
//!
//! [`super`] decides where one key goes and writes the lines that put it there. This writes the
//! files those lines live in, for a chart that does not exist yet: `values.yaml` from the image
//! block to the chassis, `templates/_helpers.tpl`, `config-contract.yaml`, `contract-tests.yaml`,
//! `ci/test-values.yaml` and a first unit-test suite. Every renderer is a pure function of the
//! [`Surface`] and a handful of names, so what a chart is scaffolded with is a property of its
//! contract rather than of the order a run happened to do things in.
//!
//! # Two forms, and the seam between them
//!
//! [`Surface`] is what makes this usable for an image that publishes no contract. Every renderer
//! branches on exactly one question — whether the surface is [`Surface::Contracted`] — rather than
//! on a flag threaded through from a caller:
//!
//! **Contracted.** The image publishes a contract, so `values.yaml` carries a block per setting with
//! the marker that binds it, `_helpers.tpl` projects them into the document, every credential is a
//! file in a Secret, and the chart is enrolled in the document, marker and round-trip gates from its
//! first commit.
//!
//! **Plain.** Nothing exists to read, so nothing is asserted about the image's configuration: no
//! `config` escape hatch, no `configMount`, no `ConfigMap` and no Secret. What is left is everything
//! that was never about a contract — the image pin and the chassis.
//!
//! The plain form is deliberately *less* than the contracted one rather than the same thing with
//! empty parts. A `configMount` in a chart whose image does not read a mounted file is a value an
//! operator can set and nothing honours; an empty `derivedConfig` is a helper that looks like a
//! mapping and maps nothing. Both would be scaffolding somebody has to notice and delete.
//!
//! # What is deliberately not generated
//!
//! **A Service, an Ingress, a route, a pod monitor or a volume claim.** Whether the workload listens,
//! on what, and whether anything should reach it is not in a contract — a `server.port` key says the
//! binary can bind a port, not that this deployment wants a Service in front of it.
//!
//! **Prose.** A chart description or README written by a generator is one nobody has read, and the
//! README generator will happily publish it.
//!
//! **Any chart file a contract does not decide.** `Chart.yaml`, the workload templates and the
//! README source belong to the repository the chart lives in, which copies them from its own
//! template; these renderers write only the files whose every line follows from the contract.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::{Map, Value as Json};

use crate::error::Error;
use crate::union::Union;

use super::super::shapes::quoted;
use super::super::testgen::VALUES_ROOT;
use super::{
    Key, Placement, Plan, WIDTH, branch_placeholder, branches, derived_for, plan_keys, render_tree,
    secret_data_for, secrets_file_name, tree_at, values_scalar, wrap, write_off_groups,
    write_off_lines,
};

/// The only document format these renderers write.
///
/// The library chart ships a TOML renderer and nothing else, so a chart whose image reads YAML or
/// JSON would need a helper the library does not have — and inventing one inside a generated
/// `_helpers.tpl` would put an untested renderer in every chart that used it.
pub const FORMAT: &str = "toml";

/// Where a chart's vendored contracts live, relative to the chart directory.
pub const CONTRACTS: &str = "contracts";

/// The `values.yaml` subtree the raw escape hatch lives under.
///
/// The round-trip generator's probe root, held as one name so the two cannot disagree about where
/// an operator's own configuration tree is.
pub const CONFIG_ROOT: &str = VALUES_ROOT;

/// A separator ruling off a section of `values.yaml`.
fn rule() -> String {
    format!("# {}", "-".repeat(WIDTH - 2))
}

// ------------------------------------------------------------------------------------------
// The surface
// ------------------------------------------------------------------------------------------

/// The spelling rules of the loader a contracted chart configures, as the generated files repeat
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialect {
    /// What the loader prepends to every variable it reads, separator included.
    pub prefix: String,
    /// What separates one path segment from the next in a variable or a secrets-file name.
    pub nesting_separator: String,
}

/// A chart's configuration surface, and where it was learnt.
///
/// See the module documentation for what each form writes and why the plain one is less rather
/// than empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Surface {
    /// The image publishes no contract.
    Plain,
    /// The image's contract, sorted into where each key goes.
    Contracted {
        /// The loader's spelling rules.
        dialect: Dialect,
        /// Every key, placed.
        plan: Plan,
    },
}

/// The plan of a surface that has none.
const EMPTY: &Plan = &Plan {
    projected: Vec::new(),
    secrets: Vec::new(),
    written_off: Vec::new(),
};

impl Surface {
    /// The contracted surface of one document's merged contract: every key, sorted.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the contract declares no nesting separator — every credential's file
    /// name and every grouped variable is spelled with it, so there is nothing to generate them
    /// from — and as [`plan_keys`].
    pub fn from_contract(union: &Union) -> Result<Self, Error> {
        let Some(separator) = union
            .dialect
            .get("nesting_separator")
            .and_then(Json::as_str)
        else {
            return Err(Error::Invalid(
                "the contract declares no nesting separator".to_owned(),
            ));
        };
        Self::from_keys(
            Dialect {
                prefix: union.prefix().to_owned(),
                nesting_separator: separator.to_owned(),
            },
            union.keys.iter().cloned().map(Key::from),
        )
    }

    /// The contracted surface of these keys, under this dialect.
    ///
    /// # Errors
    /// As [`plan_keys`].
    pub fn from_keys(dialect: Dialect, keys: impl IntoIterator<Item = Key>) -> Result<Self, Error> {
        Ok(Self::Contracted {
            dialect,
            plan: plan_keys(keys)?,
        })
    }

    /// Whether the chart's image publishes a contract.
    pub const fn contracted(&self) -> bool {
        matches!(self, Self::Contracted { .. })
    }

    /// The name of the template subtree a repository builds this surface's chart from.
    ///
    /// The two forms differ structurally — a plain chart has no configuration document, no secrets
    /// directory and no `_FILE` indirection — so a repository keeps one workload template per form
    /// rather than one with parts switched off. This is the name both sides use for each.
    pub const fn form(&self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Contracted { .. } => "contract",
        }
    }

    /// The environment prefix the image's loader owns, or the empty string when there is none.
    pub fn prefix(&self) -> &str {
        match self {
            Self::Plain => "",
            Self::Contracted { dialect, .. } => &dialect.prefix,
        }
    }

    /// Every key, placed; empty for a plain surface.
    pub fn plan(&self) -> &Plan {
        match self {
            Self::Plain => EMPTY,
            Self::Contracted { plan, .. } => plan,
        }
    }
}

/// The grouping blocks whose `# --` description is a placeholder, as values paths.
///
/// The companion to [`super::invented`], and reported for the same reason: [`branch_placeholder`]
/// writes a `TODO` because a contract says nothing about the blocks a chart groups its keys into,
/// and a placeholder nobody was told about is one the README generator publishes.
///
/// # Errors
/// As [`tree_at`].
pub fn undescribed(plan: &Plan) -> Result<Vec<String>, Error> {
    let surfaced: Vec<Placement> = plan.surfaced().cloned().collect();
    Ok(branches(&tree_at(&surfaced, 0)?, ""))
}

// ------------------------------------------------------------------------------------------
// `values.yaml`
// ------------------------------------------------------------------------------------------

/// The whole `values.yaml`: the image, the configuration surface, the chassis.
///
/// Ordered by how often it is edited rather than alphabetically. The image and the settings the
/// contract declares are what an operator opens the file for; the chassis — the blocks every chart
/// of a repository shares, passed in as text because they are the repository's and not the
/// contract's — belongs after them.
///
/// A plain surface stops after the image. Everything between it and the chassis — the settings,
/// `existingSecret`, `config`, `configExtraToml`, `configMount` — describes a loader this chart's
/// image is not known to have.
///
/// # Errors
/// As [`tree_at`] and [`super::value_block`].
pub fn render_values(
    chart: &str,
    surface: &Surface,
    repository: &str,
    tag: &str,
    chassis: &str,
    document_key: &str,
) -> Result<String, Error> {
    let mut lines = image_lines(repository, tag);
    let plan = match surface {
        Surface::Plain => {
            lines.push(String::new());
            lines.push(rule());
            lines.extend(wrap(
                "This image publishes no configuration contract, so nothing below was derived \
                 from one and nothing here is held against one. Add the values the workload needs \
                 in this section — with an `@schema` block each, which every gate in this \
                 repository expects and `values.schema.json` is generated from.",
                "# ",
                WIDTH,
            ));
            lines.push(rule());
            lines.push(String::new());
            return Ok(with_chassis(&lines, chassis));
        }
        Surface::Contracted { plan, .. } => plan,
    };
    lines.extend(settings_lines(chart, plan)?);
    lines.extend(loader_lines(chart, plan, surface.prefix(), document_key));
    Ok(with_chassis(&lines, chassis))
}

/// The file's head and the image block, which every form writes alike.
fn image_lines(repository: &str, tag: &str) -> Vec<String> {
    let mut lines: Vec<String> = [
        "# vim: set ft=yaml:",
        "# yaml-language-server: $schema=values.schema.json",
        "",
        "# @schema",
        "# additionalProperties: true",
        "# @schema",
        "# -- Container image the pod runs, composed as `registry/repository:tag`.",
        "image:",
        "  # @schema",
        "  # type: string",
        "  # @schema",
        "  # -- Registry host. Empty means Docker Hub.",
        "  registry: \"\"",
        "",
        "  # @schema",
        "  # type: string",
        "  # @schema",
        "  # -- The container image repository.",
    ]
    .map(str::to_owned)
    .into();
    lines.push(format!("  repository: {repository}"));
    lines.extend(
        [
            "",
            "  # @schema",
            "  # type: string",
            "  # @schema",
            "  # -- The container image tag. Defaults to the chart's `appVersion` when empty.",
        ]
        .map(str::to_owned),
    );
    // Always quoted. A tag like `8.0` is a YAML float unquoted, and the chart's own schema types
    // this as a string — so an unquoted numeric tag fails the first render, for a reason that reads
    // like a bug in the schema.
    lines.push(format!("  tag: {}", quoted(tag)));
    lines.extend(
        [
            "",
            "  # @schema",
            "  # enum: [\"\", Always, IfNotPresent, Never]",
            "  # @schema",
            "  # -- The image pull policy. Empty resolves automatically from the tag/digest.",
            "  pullPolicy: \"\"",
            "",
            "# @schema",
            "# type: array",
            "# @schema",
            "# -- Optional image pull secrets for private registries",
            "imagePullSecrets: []",
        ]
        .map(str::to_owned),
    );
    lines
}

/// The settings section: one tree over every key the chart surfaces, or nothing when there is none.
///
/// One tree over the settings and the credentials together, rather than a section each. They share
/// a namespace — `telemetry.log_level` is ordinary and `telemetry.sentry_dsn` is a credential — so
/// two sections would emit `telemetry:` twice and YAML would silently keep only the second,
/// dropping every value in the first. Which channel carries a value is said in its own description
/// instead, where it stays true however the keys are grouped.
///
/// # Errors
/// As [`tree_at`] and [`super::value_block`].
fn settings_lines(chart: &str, plan: &Plan) -> Result<Vec<String>, Error> {
    let mut lines = Vec::new();
    let surfaced: Vec<Placement> = plan.surfaced().cloned().collect();
    if !surfaced.is_empty() {
        lines.push(String::new());
        lines.push(rule());
        lines.extend(wrap(
            &format!(
                "The settings the image reads. `just explain {chart}` describes each one in full — \
                 its accepted values, its default, and which spelling reaches it."
            ),
            "# ",
            WIDTH,
        ));
        lines.push("#".to_owned());
        lines.extend(wrap(
            "A value marked as delivered through the secrets directory is written into a Secret \
             rather than into the ConfigMap, because a ConfigMap is readable by anything that can \
             read the namespace. `existingSecret` below replaces all of them at once.",
            "# ",
            WIDTH,
        ));
        lines.push(rule());
        lines.push(String::new());
        lines.extend(render_tree(
            &tree_at(&surfaced, 0)?,
            "",
            0,
            &branch_placeholder,
        )?);
    }
    Ok(lines)
}

/// What only a contracted chart has after its settings: `existingSecret`, the `config` escape hatch,
/// its verbatim appendix and the mount points the loader is told about.
fn loader_lines(chart: &str, plan: &Plan, prefix: &str, document_key: &str) -> Vec<String> {
    let mut lines: Vec<String> = ["", "# @schema", "# type: string", "# @schema"]
        .map(str::to_owned)
        .into();
    lines.extend(wrap(
        &format!(
            "Name of an existing Secret carrying the credentials, keeping them out of values.yaml \
             and out of the Helm release object. **Its keys are configuration paths, not \
             free-form names**: `{}`, because the file name is what the loader parses. Set, the \
             chart renders no Secret of its own and the credential values above are ignored.",
            example_secret_file(plan)
        ),
        "# -- ",
        WIDTH,
    ));
    lines.push("existingSecret: \"\"".to_owned());

    lines.extend(
        [
            "",
            "# @schema",
            "# type: object",
            "# additionalProperties: true",
            "# @schema",
        ]
        .map(str::to_owned),
    );
    lines.extend(wrap(
        &format!(
            "Extra configuration, expressed as the {} tree the image reads (`{}`, ...). Merged \
             over everything the chart derives from the values above, so it can both extend and \
             override them. Rendered into the mounted ConfigMap — never into the environment, \
             which the loader refuses to combine with a file.",
            FORMAT.to_uppercase(),
            example_key(plan)
        ),
        "# -- ",
        WIDTH,
    ));
    lines.push(format!("{CONFIG_ROOT}: {{}}"));

    lines.extend(
        [
            "",
            "# @schema",
            "# type: string",
            "# @schema",
            "# -- Verbatim TOML appended after the rendered configuration. The escape hatch for",
            "# anything the chart's TOML renderer cannot express, notably arrays of tables.",
            "configExtraToml: \"\"",
            "",
            "# @schema",
            "# additionalProperties: true",
            "# @schema",
            "# -- Where the rendered configuration document and the credential files are mounted.",
            "configMount:",
            "  # @schema",
            "  # type: string",
            "  # @schema",
        ]
        .map(str::to_owned),
    );
    lines.push(format!(
        "  # -- Directory the rendered `{document_key}` is mounted at, passed as"
    ));
    lines.push(format!("  # `{prefix}CONFIG`."));
    lines.push(format!("  configDir: /etc/{chart}/config"));
    lines.extend(
        [
            "",
            "  # @schema",
            "  # type: string",
            "  # @schema",
            "  # -- Directory the credential files are mounted at, passed as",
        ]
        .map(str::to_owned),
    );
    lines.push(format!("  # `{prefix}SECRETS_DIR`."));
    lines.push(format!("  secretsDir: /etc/{chart}/secrets"));
    lines.push(String::new());
    lines
}

/// The generated head, then the chassis with its surrounding whitespace trimmed.
fn with_chassis(lines: &[String], chassis: &str) -> String {
    format!("{}\n{}\n", lines.join("\n"), chassis.trim())
}

/// A real secrets-file name from this contract, for the `existingSecret` documentation.
fn example_secret_file(plan: &Plan) -> String {
    plan.secrets
        .iter()
        .find_map(|placement| {
            placement
                .key
                .fields
                .get("secrets_file")
                .filter(|held| !matches!(held, Json::Null | Json::Bool(false)))
                .map(|held| {
                    held.as_str()
                        .map_or_else(|| held.to_string(), str::to_owned)
                })
                .filter(|held| !held.is_empty())
        })
        .unwrap_or_else(|| "a__b".to_owned())
}

/// A real configuration path from this contract, for the `config` documentation.
fn example_key(plan: &Plan) -> &str {
    plan.projected
        .first()
        .map_or("section.setting", Placement::path)
}

// ------------------------------------------------------------------------------------------
// `templates/_helpers.tpl`
// ------------------------------------------------------------------------------------------

/// A Go-template comment block, each paragraph wrapped, an empty string a blank line.
pub fn comment(paragraphs: &[&str]) -> String {
    let mut body: Vec<String> = Vec::new();
    for paragraph in paragraphs {
        if paragraph.is_empty() {
            body.push(String::new());
        } else {
            body.extend(wrap(paragraph, "", WIDTH));
        }
    }
    format!("{{{{/*\n{}\n*/}}}}\n", body.join("\n"))
}

/// `templates/_helpers.tpl`: the partials a chart of this shape defines.
///
/// Contracted: `derivedConfig` renders the chart's own values as the tree the image reads,
/// `effectiveConfig` merges the operator's raw `config` over it so the escape hatch can both extend
/// and override, `configToml` adds the verbatim appendix, `secretData` names each credential by the
/// file name the loader parses, `secretKeys` lists those names for the pod spec, and
/// `validateValues` refuses a render that could only produce a container which fails at boot.
///
/// Plain: none of them, because every one describes a loader nothing knows this image has. A
/// comment says where a hand-written helper goes instead — a file rather than nothing, so the first
/// one is an edit and not a decision about where partials live.
///
/// # Errors
/// As [`secrets_file_name`], for a credential.
pub fn render_helpers(chart: &str, surface: &Surface) -> Result<String, Error> {
    let Surface::Contracted { dialect, plan } = surface else {
        return Ok(comment(&[
            &format!("Template partials for {chart}."),
            "",
            "Empty because this chart's image publishes no configuration contract, so there was \
             nothing to derive. Anything the templates need to compute — a volume that resolves \
             three ways, a derived configuration document, a guard that refuses an impossible \
             render — belongs here rather than inline, so the templates stay readable and the \
             computation is testable on its own.",
        ]));
    };

    let mut parts: Vec<String> = Vec::new();

    let derived = derived_for(&plan.projected, "", None, ".Values");
    parts.push(format!(
        "{}{{{{- define \"{chart}.derivedConfig\" -}}}}\n{}{{{{- end -}}}}\n",
        comment(&[
            "The configuration this chart derives from its own first-class values, as the tree \
             the image reads.",
            "",
            "Optional settings are wrapped in `with` rather than written empty. To the loader an \
             empty value is a *supplied* value, so writing one would configure the setting blank \
             rather than leaving the binary on its compiled default — which is what an operator \
             who set nothing meant.",
        ]),
        joined(&derived)
    ));

    parts.push(format!(
        "{}{{{{- define \"{chart}.effectiveConfig\" -}}}}\n\
         {{{{- $derived := include \"{chart}.derivedConfig\" . | fromYaml -}}}}\n\
         {{{{- toYaml (mergeOverwrite $derived (deepCopy (.Values.{CONFIG_ROOT} | default dict))) -}}}}\n\
         {{{{- end -}}}}\n",
        comment(&[
            "The configuration that actually reaches the image: the derived tree with the \
             operator's own `config` tree merged over it, so `config` can both extend and \
             override the values above.",
            "",
            "Not included: `configExtraToml`, which is appended verbatim and never parsed.",
        ])
    ));

    parts.push(format!(
        "{}{{{{- define \"{chart}.configToml\" -}}}}\n\
         {{{{- $config := include \"{chart}.effectiveConfig\" . | fromYaml -}}}}\n\
         {{{{- include \"common.configToml\" (dict \"ctx\" . \"maps\" (list $config)) -}}}}\n\
         {{{{- end -}}}}\n",
        comment(&[
            "The complete configuration document: the effective tree, then the verbatim escape \
             hatch."
        ])
    ));

    let secret_lines = secret_data_for(&plan.secrets)?;
    parts.push(format!(
        "{}{{{{- define \"{chart}.secretData\" -}}}}\n{}{{{{- end -}}}}\n",
        comment(&[&format!(
            "The credentials this chart manages, each keyed by the file name the loader reads it \
             from: a configuration path with `{}` for nesting and no dots, because a `.` in the \
             name is refused rather than treated as a separator.",
            dialect.nesting_separator
        )]),
        joined(&secret_lines)
    ));

    parts.push(format!(
        "{}{{{{- define \"{chart}.secretKeys\" -}}}}\n\
         {{{{- $data := include \"{chart}.secretData\" . | fromYaml -}}}}\n\
         {{{{- include \"common.fileConfig.secretKeys\" (dict \"ctx\" . \"data\" $data) -}}}}\n\
         {{{{- end -}}}}\n",
        comment(&[
            "The secret file names this pod projects, as a YAML list. Parse with `fromYamlArray`."
        ])
    ));

    parts.push(validate_values(chart, plan)?);
    Ok(parts.join("\n"))
}

/// Lines, each terminated, or nothing at all when there are none.
fn joined(lines: &[String]) -> String {
    lines.iter().fold(String::new(), |mut out, line| {
        out.push_str(line);
        out.push('\n');
        out
    })
}

/// The credentials a render cannot do without.
fn required_secrets(plan: &Plan) -> Vec<&Placement> {
    plan.secrets
        .iter()
        .filter(|item| !item.optional())
        .collect()
}

/// A guard refusing a render that could only produce a container which fails at boot.
///
/// Generated only for credentials, and checked against the *projected key list* rather than the
/// values: an `existingSecret` supplies the file without the chart being able to see inside it, and
/// a credential the chart cannot see is not the same as one that is missing.
///
/// Required non-secret keys are deliberately not guarded. They carry a default in `values.yaml` —
/// the image's own where it publishes one, a constraint-satisfying one otherwise — so a render
/// always produces a value for them, and the document gate is what holds that value to the
/// contract. A guard would only fire on a values file that had explicitly nulled one, which an
/// operator may do on purpose when a sibling layer supplies it.
fn validate_values(chart: &str, plan: &Plan) -> Result<String, Error> {
    let required = required_secrets(plan);
    if required.is_empty() {
        return Ok(format!(
            "{}{{{{- define \"{chart}.validateValues\" -}}}}\n{{{{- end -}}}}\n",
            comment(&[
                "No render is refused today: every credential this image declares is optional. \
                 Kept as a partial so the `configmap.yaml` include has something to call, and so \
                 the first required credential is one condition rather than a new file."
            ])
        ));
    }

    let mut conditions = String::new();
    for item in required {
        let file = secrets_file_name(item)?;
        let _ = write!(
            conditions,
            "{{{{- if not (has \"{file}\" $projected) -}}}}\n\
             {{{{- fail (printf \"\\n\\nVALUES VALIDATION FAILED for chart %q:\\n  - {} is required \
             unless existingSecret supplies `{file}`\\n\" .Chart.Name) -}}}}\n\
             {{{{- end -}}}}\n",
            item.values_path
        );
    }
    Ok(format!(
        "{}{{{{- define \"{chart}.validateValues\" -}}}}\n\
         {{{{- $projected := include \"{chart}.secretKeys\" . | fromYamlArray -}}}}\n\
         {conditions}{{{{- end -}}}}\n",
        comment(&[
            "Refuse a render that could only produce a container which fails at boot.",
            "",
            "Checked against the projected key list rather than against the values themselves, so \
             an `existingSecret` counts — the chart cannot see inside one, and a credential it \
             cannot see is not the same as a credential that is missing.",
        ])
    ))
}

// ------------------------------------------------------------------------------------------
// `config-contract.yaml`
// ------------------------------------------------------------------------------------------

/// The declaration the document and marker gates read.
///
/// The selector is `app.kubernetes.io/instance`, and the reason is repeated where a reader of the
/// generated file will see it: that label is `.Release.Name`, the one identifier neither
/// `nameOverride` nor `fullnameOverride` can move.
///
/// `bindings: true` is written because the scaffold emits a marker for every key it surfaces and an
/// `unbound` entry for every key it does not, so the chart is enrolled and complete from its first
/// commit rather than joining the gate later.
///
/// # Errors
/// As [`write_off_groups`].
pub fn render_declaration(
    chart: &str,
    document: &str,
    contract_file: &str,
    document_key: &str,
    plan: &Plan,
) -> Result<String, Error> {
    let mut lines: Vec<String> = [
        "# vim: set ft=yaml:",
        "",
        "# What this chart's rendered configuration must satisfy, and which image decides that.",
        "#",
        "# Scaffolded by `just new-chart`. Per-chart and self-describing, so adding a chart never",
        "# edits a central file. `just check-config` reads this, unions the contracts of every",
        "# image listed, and validates the rendered ConfigMap, the container environment and the",
        "# mounted secret file names against the result.",
        "",
        "# Whether `just check-config-bindings` holds this chart's values against the keys below.",
        "#",
        "# Declared rather than inferred from the chart carrying markers: a chart that loses them",
        "# would otherwise drop out of that gate's report without a word.",
        "bindings: true",
    ]
    .map(str::to_owned)
    .into();

    if !plan.written_off.is_empty() || !plan.secrets.is_empty() {
        lines.extend(
            [
                "",
                "# Contract keys no chart value binds, each group with the reason it does not.",
                "#",
                "# Chart-level: a key nothing surfaces is unsurfaced in every document that declares",
                "# it. `documents:` would narrow an entry where that is untrue.",
                "unbound:",
            ]
            .map(str::to_owned),
        );
        for (paths, reason) in write_off_groups(plan)? {
            lines.extend(write_off_lines(&paths, &reason));
        }
    }

    lines.extend(
        [
            "",
            "documents:",
            &format!("  - name: {document}"),
            "",
            "    # Where the rendered document is, in the output of `helm template`.",
            "    #",
            "    # Matched by label rather than by rendered name, and by `app.kubernetes.io/instance`:",
            "    # that label is `.Release.Name`, the one identifier neither `nameOverride` nor",
            "    # `fullnameOverride` can move. `just render` names the release after the chart, so",
            "    # the value below is what every rendered pair carries. A selector matching zero or",
            "    # several objects is reported rather than resolved.",
            "    source:",
            "      kind: ConfigMap",
            &format!("      selector: {{ app.kubernetes.io/instance: {chart} }}"),
            &format!("      key: {document_key}"),
            &format!("      format: {FORMAT}"),
            "",
            "    # Every binary that reads this document. One image here, so the union is the",
            "    # identity.",
            "    images:",
            "      - values: image",
            &format!("        contract: {contract_file}"),
            "",
            "    # The pods that mount it, for the environment and secret-file gates.",
            "    consumers:",
            &format!(
                "      - workload: {{ kind: Deployment, selector: {{ app.kubernetes.io/instance: \
                 {chart} }} }}"
            ),
            &format!("        containers: [{chart}]"),
            "",
            "    # Pairs this document is not fully checked for, each with a reason.",
            "    exempt: []",
        ]
        .map(str::to_owned),
    );
    Ok(format!("{}\n", lines.join("\n")))
}

/// The declaration of a chart whose first-party image publishes no contract: an explicit opt-out.
///
/// Not the same thing as having no declaration at all. The coverage gate demands a file from any
/// chart pinning a first-party image, and this is the form that satisfies it while saying, in a
/// sentence a reviewer reads, that the image publishes nothing to check against. A chart pinning a
/// third-party image owes nothing and gets nothing: a file that could only say "no" is a file nobody
/// should be asked to write.
pub fn render_opt_out(repository: &str, reason: &str) -> String {
    let mut lines: Vec<String> = [
        "# vim: set ft=yaml:",
        "",
        &format!(
            "# This chart pins {repository}, which publishes no configuration contract. It says so"
        ),
        "# here rather than simply having no file: an empty `documents` is an explicit opt-out,",
        "# and `just check-contract-coverage` reports a first-party image that is accounted for",
        "# by neither a document nor a written reason.",
        "#",
        "# `unconfigured` is deliberately absent. It would be the field for naming an image this",
        "# chart pins and does not describe — but nothing reads it on a chart with no documents,",
        "# and on a chart that has them `config_coverage` compares it against *values paths*",
        "# while `just explain` prints it as a list of images. Adding the first usage of a field",
        "# whose two readers disagree is not this file's job; the reason below names the image.",
        "",
        "documents: []",
        "",
        "reason: >-",
    ]
    .map(str::to_owned)
    .into();
    lines.extend(wrap(reason, "  ", WIDTH));
    format!("{}\n", lines.join("\n"))
}

// ------------------------------------------------------------------------------------------
// `contract-tests.yaml`
// ------------------------------------------------------------------------------------------

/// The round-trip enrolment, with a prerequisite for every credential the render demands.
///
/// A prerequisite is a chart value every generated case has to carry before the chart will render
/// at all, and the only ones known here are the credentials `validateValues` guards. Nothing here
/// may name a path under [`CONFIG_ROOT`]: a prerequisite is never dropped for the case it collides
/// with, so one written into the tree the cases probe could supply the very value a case exists to
/// prove the chart delivered. The enrolment reader refuses one outright.
///
/// # Errors
/// As [`probe_credential`] and [`secrets_file_name`].
pub fn render_enrolment(chart: &str, document: &str, plan: &Plan) -> Result<String, Error> {
    let required = required_secrets(plan);
    let mut lines: Vec<String> = [
        "# vim: set ft=yaml:",
        "",
        "# Enrols this chart in the generated configuration round-trip suites.",
        "#",
        "# `just check-config` proves the rendered document satisfies the image's contract. It",
        "# cannot prove the round trip: that a setting written into `config` arrives in that",
        "# document, at the path the image reads it from, carrying the value that was asked for.",
        "# A document missing a setting entirely satisfies the contract perfectly — every key it",
        "# does contain is legal — so a typo in a template helper passes every gate and the",
        "# container runs on a compiled default nobody chose.",
        "#",
        "# `just contract-tests` reads this file and `config-contract.yaml`, and writes one suite",
        "# per declared document under `tests/`. The presence of this file is the enrolment.",
        "documents:",
        &format!("  - name: {document}"),
    ]
    .map(str::to_owned)
    .into();

    if required.is_empty() {
        lines.extend(
            [
                "",
                "    # No `prerequisites` and no `baseline`: this chart renders without any value",
                "    # being set, so every generated case can stand on the chart's own defaults.",
            ]
            .map(str::to_owned),
        );
        return Ok(format!("{}\n", lines.join("\n")));
    }

    lines.extend(
        [
            "",
            "    # The chart values every generated case carries. This chart's `validateValues`",
            "    # fails the template before a ConfigMap exists for any case to assert against, so",
            "    # there is no case here that can do without them.",
            "    #",
            "    # Flat dotted paths, exactly as the helm-unittest `set` block they become spells",
            "    # them. Nothing here may name a path under `config`.",
            "    prerequisites:",
            "      values:",
        ]
        .map(str::to_owned),
    );
    let mut files = Vec::new();
    for item in &required {
        lines.push(format!(
            "        {}: {}",
            item.values_path,
            values_scalar(&Json::String(probe_credential(item)?))
        ));
        files.push(format!("`{}`", secrets_file_name(item)?));
    }

    lines.extend(["", "      reason: >-"].map(str::to_owned));
    lines.extend(wrap(
        &format!(
            "`{chart}.validateValues` refuses a render that projects no {} file, and it runs from \
             `configmap.yaml`, so without one there is no document for any case to assert \
             against. The guard reads the projected key list rather than the value, so \
             `existingSecret` would satisfy it as well; the first-class value is used instead \
             because it keeps the enrolment to one line and leaves the chart's Secret rendering on \
             the path every other case exercises. These occupy `secret: true` contract keys, which \
             the generator refuses to invent a probe for on the grounds that a credential-shaped \
             value in a test file is a credential — so they are written here rather than \
             synthesised, and the values are placeholders that can never authenticate anywhere.",
            files.join(", ")
        ),
        "        ",
        WIDTH,
    ));
    Ok(format!("{}\n", lines.join("\n")))
}

/// A placeholder for a required credential that cannot authenticate anywhere.
///
/// `.invalid` is reserved by RFC 2606 and can never resolve, so a value built on it is inert
/// wherever it ends up — which is the property that makes writing one into a committed test file
/// defensible at all.
///
/// # Errors
/// [`Error::Invalid`] when the credential's constraint carries a `pattern`: a placeholder that
/// satisfies an arbitrary pattern cannot be invented, so that entry has to be written by hand.
pub fn probe_credential(placement: &Placement) -> Result<String, Error> {
    let patterned = placement
        .key
        .fields
        .get("constraint")
        .and_then(Json::as_object)
        .is_some_and(|constraint| constraint.contains_key("pattern"));
    if patterned {
        return Err(Error::Invalid(format!(
            "the required credential {} constrains its text with a pattern, so a placeholder \
             cannot be invented for it; write the `prerequisites` entry by hand",
            crate::text::quoted(placement.path())
        )));
    }
    Ok(format!(
        "scaffolded-placeholder.{}.invalid",
        placement.path().replace('.', "-")
    ))
}

// ------------------------------------------------------------------------------------------
// The fixtures
// ------------------------------------------------------------------------------------------

/// `ci/test-values.yaml`: the least a fixture has to set for an install to get a running pod.
///
/// Only the credentials the render guard demands. Everything else has a default — the image's own
/// where the contract publishes one — and a fixture restating those would be a second place they
/// are written, drifting the first time one of them changed.
///
/// A plain chart has no guard and no contract to read a default out of, so its fixture is empty and
/// says so: what belongs in it is the first deliberate deviation from the chart's defaults, and only
/// the chart's author knows what that is.
///
/// # Errors
/// As [`probe_credential`].
pub fn render_ci_values(chart: &str, surface: &Surface) -> Result<String, Error> {
    let Surface::Contracted { plan, .. } = surface else {
        return Ok(format!(
            "# The default values install {chart} as they are.\n\
             #\n\
             # This chart's image publishes no configuration contract, so nothing here was\n\
             # derived from one. `ct` needs at least one fixture, and this is the place to put\n\
             # the first deliberate deviation from the chart's defaults.\n\
             {{}}\n"
        ));
    };

    let required = required_secrets(plan);
    if required.is_empty() {
        return Ok(format!(
            "# The default values install {chart} as they are: every setting the image requires\n\
             # carries a default and every credential it declares is optional. This file exists\n\
             # because `ct` needs at least one fixture, and it is the place to put the first\n\
             # deliberate deviation from the defaults.\n\
             {{}}\n"
        ));
    }

    let mut lines: Vec<String> = [
        "# The credentials this chart's render guard demands, and nothing else.",
        "#",
        "# Everything else has a default — the image's own where the contract publishes one — and",
        "# restating those here would be a second place they are written.",
        "#",
        "# `.invalid` is reserved by RFC 2606 and can never resolve, so these values are inert",
        "# wherever the installed pod ends up sending them.",
        "",
    ]
    .map(str::to_owned)
    .into();
    let mut tree = Map::new();
    for item in required {
        let segments: Vec<&str> = item.values_path.split('.').collect();
        insert_at(&mut tree, &segments, Json::String(probe_credential(item)?));
    }
    plain_yaml(&tree, "", &mut lines);
    Ok(format!("{}\n", lines.join("\n")))
}

/// Set `value` at a dotted path in a nested mapping, making each mapping on the way.
///
/// A scalar already at a branch is replaced by a mapping. The planner refuses a key whose parent is
/// another key, so for the paths this is handed that cannot happen; replacing is the answer that
/// keeps the fixture YAML if it ever did.
fn insert_at(node: &mut Map<String, Json>, segments: &[&str], value: Json) {
    match segments {
        [] => {}
        [leaf] => {
            node.insert((*leaf).to_owned(), value);
        }
        [branch, rest @ ..] => {
            let entry = node
                .entry((*branch).to_owned())
                .or_insert_with(|| Json::Object(Map::new()));
            if !entry.is_object() {
                *entry = Json::Object(Map::new());
            }
            if let Json::Object(child) = entry {
                insert_at(child, rest, value);
            }
        }
    }
}

/// A nested mapping of scalars as YAML, keys sorted. Enough for a fixture; not a general emitter.
fn plain_yaml(tree: &Map<String, Json>, indent: &str, lines: &mut Vec<String>) {
    let sorted: BTreeMap<&String, &Json> = tree.iter().collect();
    for (name, node) in sorted {
        if let Json::Object(children) = node {
            lines.push(format!("{indent}{name}:"));
            plain_yaml(children, &format!("{indent}  "), lines);
        } else {
            lines.push(format!("{indent}{name}: {}", values_scalar(node)));
        }
    }
}

/// A first hand-written suite: the `ConfigMap` renders, and it is the document it claims to be.
///
/// Deliberately thin, and deliberately not overlapping the generated round-trip suite. That one
/// proves every contract key arrives at the path the image reads it from, key by key, and
/// regenerates when the contract moves. What it cannot assert is the shape of the object around the
/// document — that a `ConfigMap` exists at all, that the escape hatch reaches the file — because
/// none of that is in the contract. This is where those go, and where a person adds the assertions
/// only they know are worth making.
///
/// # Errors
/// As [`probe_credential`].
pub fn render_unit_test(chart: &str, document_key: &str, plan: &Plan) -> Result<String, Error> {
    let mut credentials = Vec::new();
    for item in required_secrets(plan) {
        credentials.push(format!(
            "      {}: {}",
            item.values_path,
            values_scalar(&Json::String(probe_credential(item)?))
        ));
    }

    let mut lines: Vec<String> = [
        &format!("suite: {chart} configmap"),
        "templates:",
        "  - configmap.yaml",
        "",
        "# Scaffolded by `just new-chart`, and yours to extend. The generated",
        "# `contract_roundtrip_*_test.yaml` beside it proves each contract key arrives at the",
        "# path the image reads it from; this file is for everything the contract cannot say —",
        "# the shape of the object, the labels, the escape hatches.",
        "tests:",
        "  - it: renders a ConfigMap carrying the configuration document",
    ]
    .map(str::to_owned)
    .into();
    if !credentials.is_empty() {
        lines.push("    set:".to_owned());
        lines.extend(credentials.iter().cloned());
    }
    lines.extend(
        [
            "    asserts:",
            "      - hasDocuments:",
            "          count: 1",
            "      - isKind:",
            "          of: ConfigMap",
            "      - exists:",
            &format!("          path: data['{document_key}']"),
            "",
            "  - it: appends configExtraToml verbatim, after the rendered configuration",
            "    set:",
        ]
        .map(str::to_owned),
    );
    lines.extend(credentials);
    lines.extend(
        [
            "      configExtraToml: |",
            "        [scaffolded]",
            "        marker = \"verbatim\"",
            "    asserts:",
            "      - matchRegex:",
            &format!("          path: data['{document_key}']"),
            "          pattern: \"\\\\[scaffolded\\\\]\"",
        ]
        .map(str::to_owned),
    );
    Ok(format!("{}\n", lines.join("\n")))
}

#[cfg(test)]
mod tests {
    //! The whole files, over hand-built contracts rather than a registry.
    //!
    //! The rest of the scaffold's suite, ported from the generator these renderers replaced. The
    //! cases about one key — its placement, its `@schema` block, its template line — are in
    //! [`super::super`]'s own suite; these are about what only a whole file shows: that the values
    //! file is one YAML document with every value where the parser expects it, that the markers in it
    //! are ones the marker reader accepts, that every block it writes is one the README gate accepts,
    //! and that the declaration and the enrolment are ones the readers every gate uses accept.
    //!
    //! Every assertion is made through the reader that decides it rather than by matching the bytes
    //! a renderer happened to write: [`crate::helm::markers::read`] for markers,
    //! [`crate::helm::declaration::parse_declaration`] for the declaration,
    //! [`crate::helm::suites::parse_enrolment`] for the enrolment and [`crate::value::assert_value`]
    //! for an invented value. Two exceptions are written here because no Rust reader of them exists:
    //! [`blocks`], which reads `@schema` blocks the way helm-schema does, and [`undocumented`], which
    //! is `check-values-docs`' rule.

    use std::path::Path;

    use serde_json::{Map, Value as Json, json};

    use super::{
        CONFIG_ROOT, Surface, probe_credential, render_ci_values, render_declaration,
        render_enrolment, render_helpers, render_values, undescribed,
    };
    use crate::helm::declaration::{parse_declaration, read_yaml};
    use crate::helm::markers::{self, Class, Marker};
    use crate::helm::suites::parse_enrolment;
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
    /// Read the way helm-schema reads them — the text between the two delimiters is YAML, and the
    /// `# @config` marker inside it is a YAML comment — so this sees exactly what the schema
    /// generator will be handed, which is what makes an assertion about one worth anything.
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
    /// The gate's own rule, line for line — it lives in the chart repository and reads text, because
    /// the description is a comment and a YAML parser discards comments. Restated rather than
    /// re-derived: a second reader that agreed with these renderers and disagreed with the gate is
    /// the failure the assertion exists to catch.
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
        // credential is written off — so a marker on it would fail the chart's first run.
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
                .and_then(|surface| {
                    render_values("app", &surface, "org/app", "v1", "", "config.toml")
                });
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
        let text =
            "# @schema\n# type: string\n# @schema\nbare: x\n\ngroup:\n  # -- Fine.\n  leaf: 1\n";
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
        // `with` would drop a deliberate `0` or `false`, each a real setting and not "unset" — and
        // the rule holds whether the contract spells the type as a list or as a bare string.
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
        let text = render_declaration(
            "app",
            "app",
            "contracts/app.json",
            "config.toml",
            held.plan(),
        )
        .expect("the declaration renders");
        let declaration =
            parse_declaration(&text, Path::new("charts/app")).expect("the loader accepts it");
        assert!(declaration.bindings);
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

    fn enrolment(keys: &[Json]) -> Json {
        let held = surface(keys);
        let text = render_enrolment("app", "app", held.plan()).expect("the enrolment renders");
        parse_enrolment(&text, Path::new("charts/app")).expect("the enrolment reader accepts it");
        read_yaml(&text, Path::new("contract-tests.yaml")).expect("it is YAML")
    }

    #[test]
    fn a_chart_with_no_required_credential_needs_no_prerequisite() {
        let parsed = enrolment(&[
            key("a.b", &json!({})),
            key("c.token", &json!({"secret": true})),
        ]);
        assert_eq!(parsed["documents"][0].get("prerequisites"), None);
    }

    #[test]
    fn a_required_credential_becomes_a_prerequisite_with_a_reason() {
        let parsed = enrolment(&[key("a.token", &json!({"secret": true, "required": true}))]);
        let prerequisites = &parsed["documents"][0]["prerequisites"];
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
        for path in parsed["documents"][0]["prerequisites"]["values"]
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
        let value = parsed["documents"][0]["prerequisites"]["values"]["a.token"]
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
        assert!(probe_credential(&held.plan().secrets[0]).is_err());
        assert!(render_enrolment("app", "app", held.plan()).is_err());
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
}
