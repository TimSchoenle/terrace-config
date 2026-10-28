//! The declaration edits: the write-offs a chart owes, and the credential columns of its secrets
//! table.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use crate::error::Error;

use crate::helm::declaration::Declaration;
use crate::helm::regions::{Region, regions};
use crate::helm::scaffold::{
    Placement, Plan, secrets_file_name, write_off_groups, write_off_lines,
};

use super::{FileEdit, posix, read_text};

/// The block a declaration gets when it has no `unbound`, in a scaffolded declaration's wording.
pub(super) const UNBOUND_HEADER: [&str; 5] = [
    "# Contract keys no chart value binds, each group with the reason it does not.",
    "#",
    "# Chart-level: a key nothing surfaces is unsurfaced in every document that declares",
    "# it. `documents:` would narrow an entry where that is untrue.",
    "unbound:",
];

/// The block a declaration gets when it has no `credentials`.
pub(super) const CREDENTIALS_HEADER: [&str; 7] = [
    "# What this chart adds to the credential reference in its README, which",
    "# `just config-readme` generates from the contract. The rows are the contract's — every",
    "# key it marks `secret: true`, and whether the image requires it — and these columns are",
    "# the chart's: the value an operator may set instead of pre-creating the Secret, and the",
    "# condition under which a release needs the credential at all. The second is nobody's to",
    "# derive and is written by hand, as a `note:` beside the `value:`.",
    "credentials:",
];

/// The `unbound` groups and `credentials` rows this adoption owes the declaration.
///
/// **Every reason written here is read off the contract rather than decided.** That is the whole
/// licence for writing them, and it is narrower than it looks: a write-off says *why no chart value
/// surfaces this key*, and this produces exactly two answers — `reserved`, which is the image saying
/// the loader sets the key itself, and `secret`, where the standing rule sends a credential down the
/// secrets directory and never through a `ConfigMap`. Both are facts in the document. The third kind
/// of write-off — a chart choosing not to expose an ordinary setting — is a judgement, and it is
/// never produced here, because an ordinary key gets a value instead.
///
/// The `credentials` row is the same argument once more. `value` is the chart value carrying the
/// credential, which is a fact here because this just wrote that value. `note` — when a release
/// needs the credential at all — is the chart's, and is left for a person.
///
/// The second half of the answer is the blocks that exist but cannot take a line: `unbound: []`
/// and `credentials: []` are lists, but written on the key's own line, and an entry appended
/// below either is a second value for one key that no YAML parser accepts.
pub(super) fn declaration_edits(
    path: &Path,
    declaration: &Declaration,
    plan: &Plan,
    occupied: &[Placement],
) -> Result<(Vec<FileEdit>, Vec<String>), Error> {
    let text = read_text(path)?;
    // Keyed by path, a key written twice read as its last occurrence.
    let regions: HashMap<String, Region> = regions(&text)
        .into_iter()
        .map(|region| (region.values_path.clone(), region))
        .collect();

    let mut edits = Vec::new();
    let mut unwritable = Vec::new();
    let mut fresh: Vec<(&str, Vec<String>)> = Vec::new();
    let mut inline = |name: &str| {
        unwritable.push(format!(
            "{}: `{name}` is written with its value on the key's own line, so the entries this \
             adoption owes cannot be appended to it. Rewrite it as a bare `{name}:` with its \
             entries indented below, then run this again",
            posix(path)
        ));
    };

    let rows = credential_rows(declaration, plan);
    if !rows.is_empty() {
        match regions.get("credentials") {
            Some(region) if !region.bare => inline("credentials"),
            Some(region) => edits.push(FileEdit {
                path: path.to_path_buf(),
                after: region.end,
                lines: rows,
                what: "credentials".to_owned(),
            }),
            None => fresh.push(("credentials", with_header(&CREDENTIALS_HEADER, rows))),
        }
    }

    let groups = owed_write_offs(plan, occupied, declaration)?;
    if !groups.is_empty() {
        match regions.get("unbound") {
            Some(region) if !region.bare => inline("unbound"),
            Some(region) => edits.push(FileEdit {
                path: path.to_path_buf(),
                after: region.end,
                lines: groups,
                what: "unbound".to_owned(),
            }),
            None => fresh.push(("unbound", with_header(&UNBOUND_HEADER, groups))),
        }
    }

    if !fresh.is_empty() {
        let mut body = Vec::new();
        for (_, lines) in &fresh {
            body.push(String::new());
            body.extend(lines.iter().cloned());
        }
        edits.push(FileEdit {
            path: path.to_path_buf(),
            after: before_documents(&regions, path)?,
            lines: body,
            what: fresh
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(" and "),
        });
    }
    Ok((edits, unwritable))
}

pub(super) fn with_header(header: &[&str], body: Vec<String>) -> Vec<String> {
    header
        .iter()
        .map(|line| (*line).to_owned())
        .chain(body)
        .collect()
}

/// The line a block that must sit above `documents:` goes after.
///
/// The end of the last top-level key before it, rather than the line above `documents:` itself:
/// every declaration opens each of its blocks with a paragraph of prose, and an insertion between
/// that paragraph and the key it explains would attach it to the wrong one.
pub(super) fn before_documents(
    regions: &HashMap<String, Region>,
    path: &Path,
) -> Result<usize, Error> {
    let documents = regions.get("documents").ok_or_else(|| {
        Error::Invalid(format!(
            "{}: has no `documents:` key this writer can find, so there is no line to place a \
             new `unbound` or `credentials` block above",
            posix(path)
        ))
    })?;
    Ok(regions
        .values()
        .filter(|region| region.line < documents.line)
        .map(|region| region.end)
        .max()
        .unwrap_or(0))
}

/// A `credentials` row per credential this adoption surfaced and the chart has not described.
pub(super) fn credential_rows(declaration: &Declaration, plan: &Plan) -> Vec<String> {
    plan.credentials
        .iter()
        .filter(|item| !declaration.credentials.contains_key(item.path()))
        .flat_map(|item| {
            [
                format!("  - key: {}", item.path()),
                format!("    value: {}", item.values_path),
            ]
        })
        .collect()
}

/// Why a credential the chart already carries a value for is bound by no marker.
pub(super) fn occupied_reason(placement: &Placement) -> Result<String, Error> {
    Ok(format!(
        "Delivered as a file in the secrets directory — from the Secret this chart renders, or \
         from `existingSecret` under the file name `{}` — and never written into the \
         configuration document, because a credential in a ConfigMap is readable by anything that \
         can read the namespace. `just check-config-secrets` is what reconciles that channel, and \
         it does it from the rendered manifests rather than from a comment.",
        secrets_file_name(placement)?
    ))
}

/// The `unbound` groups this adoption owes, in a scaffolded declaration's layout.
///
/// A key the declaration already writes off is not written again, and a group left with no key is
/// dropped. A second entry for a key that has one is the same drift as a missing entry: one reason
/// saying what another already said, which the gate would not catch because both are true.
/// `occupied` are the credentials whose value the chart already had; they get the same sentence
/// with the clause naming the chart value removed, since that is the claim this refuses to make
/// about a value it did not write.
pub(super) fn owed_write_offs(
    plan: &Plan,
    occupied: &[Placement],
    declaration: &Declaration,
) -> Result<Vec<String>, Error> {
    let already: BTreeSet<&str> = declaration
        .unbound
        .iter()
        .flat_map(|entry| entry.keys.iter().map(String::as_str))
        .collect();

    let mut groups: Vec<(Vec<String>, String)> = write_off_groups(plan)?
        .into_iter()
        .map(|(paths, reason)| {
            let fresh: Vec<String> = paths
                .into_iter()
                .filter(|path| !already.contains(path.as_str()))
                .collect();
            (fresh, reason)
        })
        .filter(|(paths, _)| !paths.is_empty())
        .collect();
    for item in occupied {
        if !already.contains(item.path()) {
            groups.push((vec![item.path().to_owned()], occupied_reason(item)?));
        }
    }

    Ok(groups
        .iter()
        .flat_map(|(paths, reason)| write_off_lines(paths, reason))
        .collect())
}
