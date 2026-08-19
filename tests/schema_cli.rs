//! The generator's own behaviour: what it accepts, what it refuses, and what it reads back.
//!
//! The refusals carry most of the weight here. Every one of them is a case where the alternative
//! is not a crash but a *plausible* artefact — a contract missing the keys `--only` cut, a label
//! check that compared nothing and passed — and those are the failures a build log does not
//! surface.

#![cfg(feature = "schema-cli")]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use terrace_config::Terrace;
use terrace_config::schema::cli::{Cli, Format, Request, verify};
use terrace_config::schema::{
    App, ContractBuilder, DEFAULT_PATH, Describe, External, ExternalVar, JsonSchema, LABEL_PATH,
    LABEL_PREFIX, LabelFault, MARKER_BEGIN, MARKER_END, Schema,
};

#[derive(Deserialize, Serialize, Default, Describe)]
struct Config {
    /// Bundle directory the readiness probe checks.
    #[serde(default)]
    dist_dir: String,
    #[config(nested)]
    csp: Csp,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Csp {
    /// Hash the document's inline scripts.
    #[serde(default)]
    hash_inline_scripts: bool,
}

fn schema() -> Schema {
    Terrace::new("PORTFOLIO_")
        .schema::<Config>()
        .with_defaults_from(&Config::default())
        .expect("the defaults serialise")
}

fn external(builder: ContractBuilder) -> ContractBuilder {
    builder.external(External::new().var(ExternalVar::new("PORT").owner("dioxus").ty("u16")))
}

fn cli<'a>() -> Cli<'a> {
    Cli::new(App::new("portfolio").version("v1.0.0")).contract_with(&external)
}

fn args(args: &[&str]) -> Result<Request, terrace_config::schema::cli::UsageError> {
    Request::parse(args.iter().map(|arg| (*arg).to_owned()))
}

#[test]
fn the_default_request_is_the_rendering_that_loses_nothing() {
    let request = args(&[]).expect("an empty argument list is a whole request");

    assert_eq!(request.format(), Format::Json);
    assert_eq!(request.only(), "");
    // Not an argument the caller has to remember, and the same constant the `COPY` in a Dockerfile
    // uses — the two have to agree, and this is what makes agreeing the default.
    assert_eq!(request.path(), DEFAULT_PATH);
}

#[test]
fn an_argument_that_failed_to_interpolate_stops_the_build() {
    // `--version "$TAG"` with nothing in `TAG`. The alternative is a contract claiming the empty
    // release, which is worse than a build that stops: it is a build that ships.
    let error = args(&["--version", ""]).expect_err("refused");
    assert!(error.message().contains("--version"), "{error}");
    // Every message carries the usage line, because the reader is looking at a build log rather
    // than a terminal they can ask again.
    assert!(error.to_string().contains("usage:"), "{error}");

    assert!(args(&["--format"]).is_err());
    assert!(args(&["--nonsense"]).is_err());
    assert!(args(&["--format", "yaml"]).is_err());
}

#[test]
fn a_whole_image_format_refuses_a_slice_of_a_configuration() {
    // The one refusal that is about meaning rather than syntax. A contract is a claim about what
    // an image reads; built from a subset it would assert the image does not read the keys the
    // subset cut, and a validator believing that rejects a chart which is setting them correctly.
    for format in ["contract", "labels", "dockerfile"] {
        let Err(error) = args(&["--format", format, "--only", "csp"]) else {
            panic!("`--format {format}` accepted a slice");
        };
        assert!(error.message().contains("--only"), "{error}");
    }

    // The documentation renderings are the opposite case: slicing one subsystem out for a page of
    // its own is what `--only` is for.
    for format in ["json", "markdown", "toml", "json-schema"] {
        args(&["--format", format, "--only", "csp"])
            .unwrap_or_else(|error| panic!("`--format {format}` refused a slice: {error}"));
    }
}

#[test]
fn a_build_identity_is_stamped_from_the_arguments_over_the_compile_time_default() {
    let request = args(&["--format", "contract", "--revision", "abc123"]).expect("parsed");
    let rendered = cli().render(&request, schema()).expect("rendered");

    // The version the `App` was built with survives an argument list that does not mention it,
    // which is what lets `concat!("v", env!("CARGO_PKG_VERSION"))` be the usual answer.
    assert!(rendered.contains("\"version\": \"v1.0.0\""), "{rendered}");
    assert!(rendered.contains("\"revision\": \"abc123\""), "{rendered}");

    let overridden = args(&["--format", "contract", "--version", "v2.0.0"]).expect("parsed");
    let rendered = cli().render(&overridden, schema()).expect("rendered");
    assert!(rendered.contains("\"version\": \"v2.0.0\""), "{rendered}");
}

#[test]
fn a_contract_is_byte_reproducible_when_nothing_names_the_build() {
    // The property the whole design protects: a documentation job on a laptop and a container
    // build on a runner produce the same bytes, which is what lets the committed copy be diffed
    // in review. Nothing here reads a clock or the environment.
    let request = args(&["--format", "contract"]).expect("parsed");
    let first = cli().render(&request, schema()).expect("rendered");
    let second = cli().render(&request, schema()).expect("rendered");

    assert_eq!(first, second);
}

#[test]
fn the_json_schema_options_are_the_services_own() {
    let request = args(&["--format", "json-schema"]).expect("parsed");

    let bare = cli().render(&request, schema()).expect("rendered");
    let titled = cli()
        .json_schema(
            JsonSchema::new()
                .title("portfolio configuration")
                .id("urn:x"),
        )
        .render(&request, schema())
        .expect("rendered");

    assert!(!bare.contains("urn:x"), "{bare}");
    assert!(titled.contains("portfolio configuration"), "{titled}");
    assert!(titled.contains("urn:x"), "{titled}");
}

#[test]
fn the_dockerfile_block_is_cut_by_its_markers_rather_than_by_line_count() {
    let request = args(&["--format", "dockerfile"]).expect("parsed");
    let generated = cli().render(&request, schema()).expect("rendered");

    assert!(generated.starts_with(MARKER_BEGIN), "{generated}");
    assert!(generated.ends_with(MARKER_END), "{generated}");

    // A Dockerfile carrying it, with instructions on both sides that must not be picked up.
    let dockerfile = format!("FROM scratch\n{generated}\nENTRYPOINT [\"/app\"]\n");
    let extracted = verify::dockerfile_block(&dockerfile).expect("the region is there");

    assert_eq!(
        format!("{MARKER_BEGIN}\n{extracted}\n{MARKER_END}"),
        generated
    );
    assert!(!extracted.contains("FROM"), "{extracted}");
    assert!(!extracted.contains("ENTRYPOINT"), "{extracted}");
}

#[test]
fn a_region_that_could_not_be_compared_is_refused_rather_than_passed() {
    // Each of these would otherwise compare equal to nothing and report success, which is the one
    // failure this scheme cannot afford: a check that ran and looked at nothing.
    let cases = [
        ("FROM scratch\n", "no marker at all"),
        (
            &format!("FROM scratch\n{MARKER_BEGIN}\nLABEL a=\"b\"\n"),
            "opened and never closed",
        ),
        (
            &format!("{MARKER_BEGIN}\n\n{MARKER_END}\n"),
            "an empty region",
        ),
    ];

    for (dockerfile, case) in cases {
        assert!(
            verify::dockerfile_block(dockerfile).is_err(),
            "accepted {case}"
        );
    }
}

#[test]
fn the_labels_are_read_from_whichever_tool_printed_them() {
    let expected = BTreeMap::from([(
        "dev.terrace.config.prefix".to_owned(),
        "PORTFOLIO_".to_owned(),
    )]);

    // `docker inspect --format '{{json .Config.Labels}}'` — the labels object itself.
    let bare = r#"{"dev.terrace.config.prefix":"PORTFOLIO_"}"#;
    // A whole `docker inspect` config object.
    let docker = r#"{"Config":{"Labels":{"dev.terrace.config.prefix":"PORTFOLIO_"}}}"#;
    // `crane config`, which spells the same thing with a lowercase `config`.
    let crane = r#"{"config":{"Labels":{"dev.terrace.config.prefix":"PORTFOLIO_"}}}"#;

    for json in [bare, docker, crane] {
        assert_eq!(
            verify::labels_from_json(json).expect("read"),
            expected,
            "{json}"
        );
    }
}

#[test]
fn reading_the_wrong_json_path_is_a_broken_check_rather_than_a_passing_image() {
    // The classic way to make this gate pass without comparing anything: `.config.Labels` against
    // `docker inspect` output yields `null`, and a careless comparison treats that as "nothing to
    // compare". An image with no labels at all reports `{}`, so the two are distinguishable and
    // this refuses only the one that means the reader was wrong.
    let error = verify::labels_from_json("null").expect_err("refused");
    assert!(error.to_string().contains(".Config.Labels"), "{error}");

    assert!(verify::labels_from_json("[]").is_err());
    assert!(verify::labels_from_json(r#"{"a":1}"#).is_err());
    assert!(verify::labels_from_json("not json").is_err());

    // An empty label set is accepted here and fails in the comparison instead, which is where the
    // message can name the labels that are missing.
    assert!(verify::labels_from_json("{}").expect("read").is_empty());
}

#[test]
fn every_wrong_label_is_reported_rather_than_only_the_first() {
    // A build that names one missing label and hides two is a second round trip through a
    // pipeline that already took minutes.
    let contract = schema()
        .into_contract(App::new("portfolio").version("v1.0.0"))
        .build()
        .expect("built");

    let labels = verify::labels_from_json(&format!(
        r#"{{"{LABEL_PREFIX}":"WRONG_","org.opencontainers.image.title":"portfolio"}}"#
    ))
    .expect("read");

    let faults = contract.check_labels(DEFAULT_PATH, &labels);

    // All three: two never pasted and one gone stale. Reported in the contract's own declaration
    // order rather than in the order the image happened to list them, so two runs against two
    // images read the same way.
    assert_eq!(faults.len(), 3, "{faults:?}");
    assert!(matches!(
        faults[1],
        LabelFault::Missing { name } if name == LABEL_PATH
    ));
    assert!(matches!(
        faults[2],
        LabelFault::Mismatch { name, .. } if name == LABEL_PREFIX
    ));

    // `verify_labels` is the same check with the reporting decided, and it names both.
    let error = contract
        .verify_labels(DEFAULT_PATH, &labels)
        .expect_err("refused");
    assert!(error.to_string().contains(LABEL_PATH), "{error}");
    assert!(error.to_string().contains(LABEL_PREFIX), "{error}");
}

#[test]
fn what_a_build_generates_is_what_a_built_image_is_checked_against() {
    // The round trip the six shell scripts were each half of: render the labels, pretend an image
    // carried them plus the ones a base image contributes, and check.
    let request = args(&["--format", "labels"]).expect("parsed");
    let rendered = cli().render(&request, schema()).expect("rendered");

    let mut labels: BTreeMap<String, String> = rendered
        .lines()
        .map(|line| {
            let (name, value) = line.split_once('=').expect("NAME=value");
            (name.to_owned(), value.to_owned())
        })
        .collect();
    labels.insert(
        "org.opencontainers.image.title".to_owned(),
        "portfolio".to_owned(),
    );

    let contract = external(schema().into_contract(App::new("portfolio").version("v1.0.0")))
        .build()
        .expect("built");

    contract
        .verify_labels(DEFAULT_PATH, &labels)
        .expect("the labels this generator emitted are the ones the contract accepts");
}
