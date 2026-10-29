//! The subcommands that read one contract document: `validate`, `conform`, `render` and `stamp`.

use std::process::ExitCode;

use terrace_contract::render::{self, Column, Docs, Format, JsonSchema, Options, TomlExample};
use terrace_contract::{Contract, Error, Tier, conform, validate};

use crate::args::{Command, Input};
use crate::commands::image::{check_dockerfile, exit_for, write_dockerfile};
use crate::output::report_lines;

/// Hold a document to the published meta-schema.
pub(crate) fn validate_command(input: &Input) -> Result<ExitCode, Error> {
    let errors = validate::validate(&input.read_text()?)?;
    Ok(report_lines(
        &errors,
        "the document does not satisfy `spec/v1/contract.schema.json`:",
    ))
}

pub(crate) fn conform_command(
    input: &Input,
    tier: Tier,
    no_schema: bool,
) -> Result<ExitCode, Error> {
    let text = input.read_text()?;

    // The meta-schema first, and its failures are not merged with the rule failures below. A
    // document that is not the right *shape* produces rule violations that are artefacts of the
    // misreading, and a producer's author chasing those is chasing the wrong bug.
    if !no_schema {
        let errors = validate::validate(&text)?;
        if !errors.is_empty() {
            return Ok(report_lines(
                &errors,
                "the document does not satisfy `spec/v1/contract.schema.json`, so the rules \
                 below it were not run:",
            ));
        }
    }

    let contract = Contract::from_json(&text)?;
    if let Some(ahead) = contract.schema_version_ahead() {
        eprintln!(
            "note: the document declares `schema_version` {ahead}, and this build \
             understands {}. Every field this build knows still means what it \
             meant; there may be more here than was checked.",
            terrace_contract::SCHEMA_VERSION
        );
    }

    let violations: Vec<String> = conform::conform(&contract, tier)
        .into_iter()
        .map(|violation| violation.to_string())
        .collect();

    Ok(report_lines(
        &violations,
        &format!("the document does not conform at tier {tier}:"),
    ))
}

pub(crate) fn render_command(command: Command) -> Result<ExitCode, Error> {
    let Command::Render {
        input,
        format,
        path,
        columns,
        title,
        id,
        open,
        no_header,
        full_docs,
        write,
        check,
    } = command
    else {
        unreachable!("dispatched on the variant")
    };

    let contract = input.read()?;
    if let Some(dockerfile) = write.as_deref().or(check.as_deref()) {
        if format != Format::Dockerfile {
            return Err(Error::Invalid(
                "--write and --check edit or compare a Dockerfile's label regions, so they need \
                 --format dockerfile"
                    .to_owned(),
            ));
        }
        return if write.is_some() {
            write_dockerfile(dockerfile, &contract, &path)
        } else {
            Ok(exit_for(check_dockerfile(dockerfile, &contract, &path)?))
        };
    }
    let columns = if columns.is_empty() {
        Column::DEFAULT.to_vec()
    } else {
        columns
    };
    let options = Options {
        path: &path,
        columns: &columns,
        json_schema: JsonSchema {
            title,
            id,
            closed: !open,
            ..JsonSchema::default()
        },
        toml_example: TomlExample {
            header: !no_header,
            docs: if full_docs { Docs::Full } else { Docs::Summary },
            ..TomlExample::default()
        },
    };

    // Exactly one trailing newline, whatever the rendering ended with, because a build redirects
    // this into a committed file and a trailing blank line is invisible on a terminal and a diff
    // in the file.
    print!(
        "{}",
        render::one_newline(render::render(&contract, format, &options)?)
    );
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn stamp_command(command: Command) -> Result<ExitCode, Error> {
    let Command::Stamp {
        input,
        app_version,
        revision,
        created,
        source,
    } = command
    else {
        unreachable!("dispatched on the variant")
    };

    let mut contract = input.read()?;
    // Each field is set only when given, so stamping twice with different arguments accumulates
    // rather than clearing what the first run wrote.
    if app_version.is_some() {
        contract.app.version = app_version;
    }
    if revision.is_some() {
        contract.app.revision = revision;
    }
    if created.is_some() {
        contract.app.created = created;
    }
    if source.is_some() {
        contract.app.source = source;
    }

    print!(
        "{}",
        render::one_newline(render::render(
            &contract,
            Format::Contract,
            &Options::default()
        )?)
    );
    Ok(ExitCode::SUCCESS)
}
