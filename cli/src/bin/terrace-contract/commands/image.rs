//! The `image` subcommands: checking a built image against its contract, and attaching one.

use std::path::Path;
use std::process::ExitCode;

use terrace_contract::render::image;
use terrace_contract::{Contract, Error, oci};

use crate::args::{Input, read};

/// What `image verify` was asked to compare the document against.
pub(crate) struct Verify<'a> {
    pub(crate) labels: Option<&'a Path>,
    pub(crate) dockerfile: Option<&'a Path>,
    pub(crate) image: Option<&'a str>,
    pub(crate) oci_layout: bool,
    pub(crate) signer: Option<&'a str>,
    pub(crate) path: &'a str,
}

pub(crate) fn verify_command(input: &Input, verify: &Verify<'_>) -> Result<ExitCode, Error> {
    if verify.labels.is_none() && verify.dockerfile.is_none() && verify.image.is_none() {
        return Err(Error::Invalid(
            "nothing to verify against: pass --image, --labels, --dockerfile, or a combination. A \
             verify with none would report success without comparing anything, which is the one \
             outcome this check cannot afford."
                .to_owned(),
        ));
    }

    let document = input.read_text()?;
    let contract = Contract::from_json(&document)?;
    let mut failed = false;

    if let Some(path_to_labels) = verify.labels {
        let found = image::labels_from_json(&read(path_to_labels)?)?;
        if let Some(report) = image::report(&image::check_labels(&contract, verify.path, &found)) {
            eprintln!("{report}");
            failed = true;
        }
    }

    if let Some(reference) = verify.image {
        let reference = oci::Reference::parse(reference, verify.oci_layout)?;
        let tools = oci::Tools::reading()?;
        let tools = if verify.oci_layout {
            tools.in_layout()
        } else {
            tools
        };
        let digest = reference.digest(&tools)?;
        let faults = oci::verify_image(
            &tools,
            &reference.repository,
            &digest,
            document.as_bytes(),
            &contract,
            verify.path,
            verify.signer,
        )?;
        if faults.is_empty() {
            println!(
                "==> {}@{digest} publishes this document",
                reference.repository
            );
        } else {
            eprintln!(
                "{}@{digest} does not publish this document:",
                reference.repository
            );
            for fault in &faults {
                eprintln!("  {fault}");
            }
            failed = true;
        }
    }

    if let Some(path_to_dockerfile) = verify.dockerfile {
        failed |= check_dockerfile(path_to_dockerfile, &contract, verify.path)?;
    }

    Ok(exit_for(failed))
}

/// 1, not 2, for a check that ran and found something wrong: the tool worked and the input is wrong.
pub(crate) fn exit_for(failed: bool) -> ExitCode {
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Compares every marked region of a Dockerfile with the block this document renders, reporting
/// each stale or empty one; returns whether any was.
pub(crate) fn check_dockerfile(
    dockerfile: &Path,
    contract: &Contract,
    path: &str,
) -> Result<bool, Error> {
    let text = read(dockerfile)?;
    let expected = image::dockerfile_labels(contract, path);
    // Every region is compared: a Dockerfile building several images carries one per stage, and
    // each is a place the contract is published from.
    let regions = image::committed_blocks(&text).map_err(|error| named(dockerfile, error))?;
    let mut failed = false;
    for region in regions {
        // A Dockerfile checked out with CRLF would otherwise never match a block rendered with LF,
        // and the diff would be invisible in every terminal that shows it.
        let committed = region.block.replace("\r\n", "\n");
        if committed.is_empty() {
            eprintln!(
                "{}:{}: the `{}` region is empty, so it publishes no labels. Run `render --format \
                 dockerfile --write` to fill it.",
                dockerfile.display(),
                region.line,
                terrace_contract::document::MARKER_BEGIN
            );
            failed = true;
        } else if committed.trim_end() != expected.trim_end() {
            eprintln!(
                "{}:{}: the committed label block is not the one this document renders.\n\
                 --- committed\n{committed}\n\
                 --- expected\n{}",
                dockerfile.display(),
                region.line,
                expected.trim_end()
            );
            failed = true;
        }
    }
    Ok(failed)
}

/// Rewrites every marked region of a Dockerfile with the block this document renders.
pub(crate) fn write_dockerfile(
    dockerfile: &Path,
    contract: &Contract,
    path: &str,
) -> Result<ExitCode, Error> {
    let text = read(dockerfile)?;
    let rewritten =
        image::rewrite_blocks(&text, contract, path).map_err(|error| named(dockerfile, error))?;
    if rewritten == text {
        println!(
            "==> {}: every label region is current",
            dockerfile.display()
        );
    } else {
        std::fs::write(dockerfile, rewritten).map_err(|e| Error::io(dockerfile.display(), e))?;
        println!("==> {}: label regions rewritten", dockerfile.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// A refusal from the region parser, which only knows lines, prefixed with the file it read: a
/// message that does not say which file is no help in a build checking several.
pub(crate) fn named(dockerfile: &Path, error: Error) -> Error {
    match error {
        Error::Invalid(message) => Error::Invalid(format!("{}: {message}", dockerfile.display())),
        other => other,
    }
}

/// Attaches the document to the image's digest, unless that digest already carries it.
pub(crate) fn attach_command(
    contract: &Path,
    reference: &str,
    oci_layout: bool,
) -> Result<ExitCode, Error> {
    let document = std::fs::read(contract).map_err(|e| Error::io(contract.display(), e))?;
    // Read before it is published: a referrer is permanent in the sense that matters, since every
    // later attach of a corrected document would leave the digest carrying two contracts.
    let text = String::from_utf8(document.clone()).map_err(|failure| {
        Error::Invalid(format!("{}: not UTF-8: {failure}", contract.display()))
    })?;
    Contract::from_json(&text)?;

    let reference = oci::Reference::parse(reference, oci_layout)?;
    let tools = oci::Tools::reading()?;
    let tools = if oci_layout { tools.in_layout() } else { tools };
    let digest = reference.digest(&tools)?;
    let pinned = format!("{}@{digest}", reference.repository);

    match oci::fetch_referrer(&tools, &reference.repository, &digest)? {
        Some(held) if held.payload == document => {
            eprintln!("==> {pinned} already carries this document");
            println!("{}", held.digest);
            Ok(ExitCode::SUCCESS)
        }
        Some(_) => Err(Error::Invalid(format!(
            "{pinned} already carries a different contract as a referrer. A digest with two \
             contracts has none a consumer can trust, so this one is not attached: the image and \
             the document came from different builds."
        ))),
        None => {
            let attached = tools.attach(&pinned, contract)?;
            eprintln!("==> attached the contract to {pinned}");
            println!("{attached}");
            Ok(ExitCode::SUCCESS)
        }
    }
}
