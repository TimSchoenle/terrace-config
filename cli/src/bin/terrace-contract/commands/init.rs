//! `helm init`, which writes the configuration half of a new chart.

use std::process::ExitCode;

use terrace_contract::repo::Settings;
use terrace_contract::{Error, helm, oci};

use crate::args::{Init, read};
use crate::settings::{Repo, required_signer};

/// Plan, check, diff or write the configuration half of one new chart.
///
/// The exit codes follow `adopt`'s: 1 for any failure, except under `--check`, where 1 means
/// "something would be written" and a failure to answer is 2.
pub(crate) fn helm_init_command(repo: &Repo, options: Init) -> ExitCode {
    use helm::init::{self, Pin, Request, Source};

    let check = options.check;
    let run = || -> Result<ExitCode, Error> {
        let settings = repo.settings()?;
        let charts = settings.charts(options.charts).0;
        let pin = Pin::parse(&options.image)?;
        let chassis = options
            .chassis
            .as_deref()
            .map(read)
            .transpose()?
            .unwrap_or_default();
        let request = Request {
            chart: &options.chart,
            pin: &pin,
            document: options.document.as_deref().unwrap_or(&options.chart),
            document_key: &options.document_key,
            chassis: &chassis,
        };

        let fetched;
        let at;
        let source = if options.no_contract {
            Source::Plain {
                opt_out: no_contract_opt_out(&settings, &pin, options.reason.as_deref())?,
            }
        } else {
            let signer = required_signer(&settings, options.signer)?;
            let tools = oci::Tools::found()?;
            fetched = helm::pull::fetch_pinned(&tools, &pin.block(), None, &signer).map_err(
                |failure| {
                    Error::Invalid(format!(
                        "{failure}\n       pass --no-contract (and --reason, for a first-party \
                         image) if this image publishes none"
                    ))
                },
            )?;
            at = helm::pull::now();
            Source::Contract {
                fetched: &fetched,
                at: &at,
            }
        };

        let planned = init::plan(&charts, &request, &source)?;
        if options.diff {
            print!("{}", planned.diff());
            return Ok(ExitCode::SUCCESS);
        }
        print!("{}", planned.report());
        if options.write && planned.owed() {
            planned.write()?;
            print!("{}", planned.closing());
        }
        Ok(if check && planned.owed() {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        })
    };

    run().unwrap_or_else(|failure| {
        eprintln!("error: {failure}");
        ExitCode::from(if check { 2 } else { 1 })
    })
}

/// The opt-out a plain chart's declaration carries, when the coverage gate will demand one.
///
/// Decided from the same first-party list the gate reads, and refused rather than guessed when no
/// list resolves: a chart pinning a first-party image owes a declaration, and an opt-out without a
/// reason is indistinguishable from a chart nobody got round to declaring.
pub(crate) fn no_contract_opt_out<'a>(
    settings: &Settings,
    pin: &helm::init::Pin,
    reason: Option<&'a str>,
) -> Result<Option<&'a str>, Error> {
    let (patterns, _) = settings.first_party(None).map_err(|failure| {
        Error::Invalid(format!(
            "no first-party image list resolved, so whether this chart owes a declaration cannot \
             be decided ({failure}); set `helm.firstParty` in terrace.yaml (`terrace-contract \
             doctor` says why)"
        ))
    })?;
    if !helm::init::owes_a_declaration(&patterns, &pin.repository) {
        return Ok(None);
    }
    match reason.filter(|held| !held.trim().is_empty()) {
        Some(reason) => Ok(Some(reason)),
        None => Err(Error::Invalid(format!(
            "{} is a first-party image, so the coverage gate will demand a declaration from this \
             chart. Give --reason to write the opt-out, or drop --no-contract to vendor the \
             contract it publishes",
            pin.repository
        ))),
    }
}
