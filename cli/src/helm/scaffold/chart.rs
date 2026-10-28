//! The whole configuration files a new chart starts with, composed from the planner's rules.
//!
//! [`super`] decides where one key goes and writes the lines that put it there. This writes the
//! files those lines live in, for a chart that does not exist yet: `values.yaml` from the image
//! block to the chassis, `templates/_helpers.tpl`, `config-contract.yaml` (with the round-trip
//! enrolment in it), `ci/test-values.yaml` and a first unit-test suite. Every renderer is a pure function of the
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
use crate::gate::DocumentFormat;
use crate::union::Union;

use super::super::declaration::{
    CONTRACTS_DIR, DECLARATION_SCHEMA_URL, DEFAULT_SOURCE_KEY, format_of,
};
use super::super::shapes::quoted;
use super::super::testgen::VALUES_ROOT;
use super::{
    Key, Placement, Plan, WIDTH, branch_placeholder, branches, credential_entries_for, derived_for,
    plan_keys, render_tree, secrets_file_name, tree_at, values_scalar, wrap, write_off_groups,
    write_off_lines,
};

/// The only document format these renderers write.
///
/// The library chart ships a TOML renderer and nothing else, so a chart whose image reads YAML or
/// JSON would need a helper the library does not have — and inventing one inside a generated
/// `_helpers.tpl` would put an untested renderer in every chart that used it.
pub const FORMAT: &str = "toml";

/// Where a chart's vendored contracts live, relative to the chart directory.
pub const CONTRACTS: &str = CONTRACTS_DIR;

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
    credentials: Vec::new(),
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
    plan.credentials
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

    let secret_lines = credential_entries_for(&plan.credentials)?;
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
    plan.credentials
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

/// The declaration the document, marker and round-trip gates read.
///
/// Only what differs from the defaults in `spec/helm/v1/DEFAULTS.md` is written: the document's
/// name, its key when it is not `config.toml`, and how its round-trip suite is generated when the
/// render needs prerequisites. Everything a scaffolded chart shares with every other one — the
/// release selector, the image at `image` with its contract under the document's name, the
/// workload that mounts the `ConfigMap` — is left to the defaults, so the file a person reads is
/// the part that is about this chart.
///
/// `bindings: true` is written because the scaffold emits a marker for every key it surfaces and an
/// `unbound` entry for every key it does not, so the chart is enrolled and complete from its first
/// commit rather than joining the gate later. `tests: true` is written for the same reason: every
/// prerequisite a generated case needs is known here.
///
/// # Errors
/// As [`write_off_groups`], [`probe_credential`] and [`secrets_file_name`].
pub fn render_declaration(
    chart: &str,
    document: &str,
    document_key: &str,
    plan: &Plan,
) -> Result<String, Error> {
    let mut lines: Vec<String> = [
        &format!("# yaml-language-server: $schema={DECLARATION_SCHEMA_URL}"),
        "#",
        "# What this chart's rendered configuration must satisfy, and which image decides that.",
        "# A field left out takes its default from spec/helm/v1/DEFAULTS.md in",
        &format!(
            "# TimSchoenle/terrace-config; `terrace-contract config show --chart {chart}` prints \
             the"
        ),
        "# declaration every gate reads.",
        "",
        "# Held against the `@config` markers in values.yaml, and enrolled in the round-trip suites.",
        "bindings: true",
        "tests: true",
    ]
    .map(str::to_owned)
    .into();

    if !plan.written_off.is_empty() || !plan.credentials.is_empty() {
        lines.extend(
            [
                "",
                "# Contract keys no chart value binds, each group with the reason it does not.",
                "unbound:",
            ]
            .map(str::to_owned),
        );
        for (paths, reason) in write_off_groups(plan)? {
            lines.extend(write_off_lines(&paths, &reason));
        }
    }

    lines.extend(["", "documents:", &format!("  - name: {document}")].map(str::to_owned));
    // The scaffold always renders TOML, so a key whose extension implies another format says so.
    if format_of(document_key) != DocumentFormat::Toml {
        lines.push(format!(
            "    source: {{ key: {document_key}, format: {FORMAT} }}"
        ));
    } else if document_key != DEFAULT_SOURCE_KEY {
        lines.push(format!("    source: {{ key: {document_key} }}"));
    }
    lines.extend(tests_lines(chart, plan)?);
    Ok(format!("{}\n", lines.join("\n")))
}

/// A document's `tests` block, with a prerequisite for every credential the render demands; nothing
/// when the chart renders on its own defaults.
///
/// A prerequisite is a chart value every generated case has to carry before the chart will render
/// at all, and the only ones known here are the credentials `validateValues` guards. Nothing here
/// may name a path under [`CONFIG_ROOT`]: a prerequisite is never dropped for the case it collides
/// with, so one written into the tree the cases probe could supply the very value a case exists to
/// prove the chart delivered. The enrolment reader refuses one outright.
fn tests_lines(chart: &str, plan: &Plan) -> Result<Vec<String>, Error> {
    let required = required_secrets(plan);
    if required.is_empty() {
        return Ok(Vec::new());
    }

    let mut lines: Vec<String> = [
        "",
        "    # The chart values every generated round-trip case carries: `validateValues` fails the",
        "    # template before a ConfigMap exists for any case to assert against without them.",
        "    tests:",
        "      prerequisites:",
        "        values:",
    ]
    .map(str::to_owned)
    .into();
    let mut files = Vec::new();
    for item in &required {
        lines.push(format!(
            "          {}: {}",
            item.values_path,
            values_scalar(&Json::String(probe_credential(item)?))
        ));
        files.push(format!("`{}`", secrets_file_name(item)?));
    }

    lines.push("        reason: >-".to_owned());
    lines.extend(wrap(
        &format!(
            "`{chart}.validateValues` refuses a render that projects no {} file, and it runs from \
             `configmap.yaml`, so without one there is no document for any case to assert \
             against. The guard reads the projected key list rather than the value, so \
             `existingSecret` would satisfy it as well; the first-class value is used instead \
             because it keeps the prerequisite to one line and leaves the chart's Secret \
             rendering on the path every other case exercises. These occupy `secret: true` \
             contract keys, which the generator refuses to invent a probe for on the grounds that \
             a credential-shaped value in a test file is a credential — so they are written here \
             rather than synthesised, and the values are placeholders that can never authenticate \
             anywhere.",
            files.join(", ")
        ),
        "          ",
        WIDTH,
    ));
    Ok(lines)
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
        &format!("# yaml-language-server: $schema={DECLARATION_SCHEMA_URL}"),
        "#",
        &format!(
            "# This chart pins {repository}, which publishes no configuration contract. It says so"
        ),
        "# here rather than simply having no file: an empty `documents` is an explicit opt-out,",
        "# and the coverage gate reports a first-party image that is accounted for by neither a",
        "# document nor a written reason.",
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
mod tests;
