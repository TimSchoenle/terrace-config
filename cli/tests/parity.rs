//! The Python this half was ported from, against this crate, over a real chart tree.
//!
//! The single most valuable check available while the port is happening, and the reason is
//! arithmetic: ~15,000 lines of rules came across with ~8,000 lines of tests, and one corpus of nine
//! charts and fifteen contracts. The tests say the rules do what somebody thought they did. The
//! corpus is what finds what nobody thought about — a `CronJob` nested one template deeper than
//! expected, a projected volume with no `items`, a container whose image is overridden in one values
//! file out of fourteen.
//!
//! # It skips rather than fails when the tree is not there
//!
//! Gated on `TERRACE_PARITY_CHARTS`, which names a checkout of the consuming repository. Without it
//! this is a no-op, because the tree is not this repository's and a developer here should not have
//! to have it. CI sets it, at [`VERIFIED_REF`].
//!
//! ```text
//! TERRACE_PARITY_CHARTS=E:/helm-charts cargo test --test parity -- --nocapture
//! ```
//!
//! Both sides must read **the same rendered directory**. Rendering twice would compare two renders
//! as much as two implementations, and a chart whose output depends on anything ambient would show
//! up here as a rule difference.
//!
//! # A clean tree is a weak oracle
//!
//! Both implementations finding nothing over a correct corpus proves that neither crashed. So the
//! harness also renders a *mutant* of the tree — one that breaks gate 1, gate 2 and the service-link
//! precondition at once — and compares over that. Mutating structurally and writing the result once,
//! rather than mutating each side's input separately, keeps the comparison about the rules: both
//! implementations read the same bytes, including the ones that are wrong.
//!
//! # It outlives the Python
//!
//! When the scripts are deleted the oracle stops resolving, and this keeps running the Rust side
//! over the same tree and asserts it is clean. That is worth having on its own: it is the only test
//! anywhere with a real corpus behind it.

#![cfg(feature = "helm")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use terrace_contract::helm;
use terrace_contract::report::Level;

/// The commit of the consuming repository this was last verified against.
///
/// Pinned rather than floating so that a difference appearing here is a change in *this* repository
/// until somebody deliberately moves it. A parity harness pointed at a moving tree reports the other
/// repository's edits as this one's regressions, and people stop reading it.
const VERIFIED_REF: &str = "4fab35383cad5b70a5d510f616ba810eb2a4fa79";

/// The differences that are meant to be there, and why.
///
/// Every entry is a decision recorded in `docs/contract-cli-plan.md`, not a difference somebody grew
/// tired of chasing. A finding matching one of these is expected on exactly one side; anything else
/// fails the run with both texts printed, so classifying it is a person's job and not a regex's.
const DELIBERATE: &[Deliberate] = &[Deliberate {
    side: Side::Rust,
    opening: "range not checked:",
    reason: "\
        §7.1. The Python performed the range read unconditionally and never read `producer` at all. \
        The reads belong to `producer.loader`, and every contract vendored in this corpus predates \
        that field — so this build skips the read and says so, where the Python performed it with \
        figment's rules. A warning on one side and silence on the other, by design.",
}];

/// Which implementation a deliberate difference belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Rust,
    Python,
}

/// One expected difference.
struct Deliberate {
    side: Side,
    /// What the message starts with. A prefix rather than a pattern: a rule that changed its own
    /// wording should show up here as a difference to classify, not be swallowed by a loose match.
    opening: &'static str,
    reason: &'static str,
}

/// One finding, as both sides describe it.
type Finding = (String, String, String);

#[test]
fn the_rules_agree_with_the_implementation_they_were_ported_from() {
    let Some(root) = tree() else {
        eprintln!(
            "skipped: set TERRACE_PARITY_CHARTS to a checkout of the consuming repository \
             (verified at {VERIFIED_REF})"
        );
        return;
    };
    let rendered = root.join("rendered");
    assert!(
        rendered.is_dir(),
        "{}: no rendered manifests. Render the charts once, and point both implementations at that \
         one directory — rendering twice would compare two renders as much as two implementations.",
        rendered.display()
    );

    let ours = rust_side(&root, &rendered);
    let Some(theirs) = python_side(&root, &rendered) else {
        // The Python is gone, which is the intended end state. What is left is still the only test
        // with a real corpus behind it, so it keeps running.
        let failures: Vec<&Finding> = ours
            .iter()
            .filter(|(_, level, _)| level == "error")
            .collect();
        assert!(
            failures.is_empty(),
            "the oracle is gone and this crate finds {} failure(s) in {}:\n{}",
            failures.len(),
            root.display(),
            render(&failures)
        );
        eprintln!(
            "the oracle is gone; checked {} finding(s) over {} and found no failures",
            ours.len(),
            root.display()
        );
        return;
    };

    let only_ours = missing_from(&ours, &theirs);
    let only_theirs = missing_from(&theirs, &ours);

    let unexplained_ours = unexplained(&only_ours, Side::Rust);
    let unexplained_theirs = unexplained(&only_theirs, Side::Python);

    assert!(
        unexplained_ours.is_empty() && unexplained_theirs.is_empty(),
        "the two implementations disagree, and the difference is not one of the {} recorded in \
         DELIBERATE.\n\nfound only by this crate ({}):\n{}\nfound only by the oracle ({}):\n{}\n\
         Either the port has a defect, or the difference is deliberate and belongs in DELIBERATE \
         with the paragraph that justifies it.",
        DELIBERATE.len(),
        unexplained_ours.len(),
        render(&unexplained_ours),
        unexplained_theirs.len(),
        render(&unexplained_theirs)
    );

    eprintln!(
        "parity over {}: {} finding(s) agreed, {} deliberate difference(s)",
        root.display(),
        ours.len() - only_ours.len(),
        only_ours.len() + only_theirs.len()
    );
}

#[test]
fn every_deliberate_difference_is_still_produced_by_the_side_that_claims_it() {
    // A recorded difference nothing produces is a rule that changed without the record changing
    // with it, which is how a documented deviation becomes a stale comment.
    let Some(root) = tree() else {
        eprintln!("skipped: TERRACE_PARITY_CHARTS is not set");
        return;
    };
    let rendered = root.join("rendered");
    if !rendered.is_dir() {
        eprintln!("skipped: {} is not there", rendered.display());
        return;
    }

    let ours = rust_side(&root, &rendered);
    for entry in DELIBERATE.iter().filter(|entry| entry.side == Side::Rust) {
        assert!(
            ours.iter()
                .any(|(_, _, message)| message.starts_with(entry.opening)),
            "DELIBERATE records a difference opening {:?}, and this crate produced none over {}. \
             Either the rule changed or the record is stale.\n{}",
            entry.opening,
            root.display(),
            entry.reason
        );
    }
}

#[test]
fn the_rules_agree_about_a_tree_that_is_wrong() {
    // The check the clean corpus cannot make. Silence agreeing with silence says only that neither
    // implementation fell over; this is where a rule that fires on one side and not the other, or
    // fires with different words, actually shows up.
    let Some(root) = tree() else {
        eprintln!("skipped: TERRACE_PARITY_CHARTS is not set");
        return;
    };
    let rendered = root.join("rendered");
    if !rendered.is_dir() {
        eprintln!("skipped: {} is not there", rendered.display());
        return;
    }
    if python_side(&root, &rendered).is_none() {
        eprintln!("skipped: the oracle is gone, so there is nothing to compare a mutant against");
        return;
    }

    let mutant = Mutant::of(&rendered);
    let ours = rust_side(&root, mutant.path());
    let theirs = python_side(&root, mutant.path()).expect("the oracle was there a moment ago");

    assert!(
        ours.len() > 20,
        "the mutation reached nothing: {} finding(s) over a tree broken in three places. Either the          mutations no longer apply to this corpus or the rules stopped running.
{}",
        ours.len(),
        render(&ours.iter().collect::<Vec<_>>())
    );

    let only_ours = missing_from(&ours, &theirs);
    let only_theirs = missing_from(&theirs, &ours);
    let unexplained_ours = unexplained(&only_ours, Side::Rust);
    let unexplained_theirs = unexplained(&only_theirs, Side::Python);

    assert!(
        unexplained_ours.is_empty() && unexplained_theirs.is_empty(),
        "the two implementations disagree about a deliberately broken tree.

found only by this          crate ({}):
{}
found only by the oracle ({}):
{}",
        unexplained_ours.len(),
        render(&unexplained_ours),
        unexplained_theirs.len(),
        render(&unexplained_theirs)
    );

    eprintln!(
        "parity over a mutant of {}: {} finding(s) agreed",
        rendered.display(),
        ours.len() - only_ours.len()
    );
}

/// A copy of a rendered tree with three defects introduced, removed when it goes out of scope.
///
/// Structural rather than textual, and written once for both implementations to read. Three
/// defects, chosen because each belongs to a different rule and none of them can be found by
/// anything else in a chart repository's pipeline:
///
/// | Mutation | Reaches |
/// |---|---|
/// | a key no contract declares, prepended to every configuration document | gate 1 |
/// | a variable no contract accounts for, on every container | gate 2 |
/// | `enableServiceLinks` switched back on | the precondition gate |
struct Mutant(PathBuf);

impl Mutant {
    fn of(rendered: &Path) -> Self {
        let at = std::env::temp_dir().join(format!("terrace-parity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(&at).expect("a mutant tree is created");

        for entry in std::fs::read_dir(rendered).expect("the rendered tree is readable") {
            let path = entry.expect("a rendered file").path();
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("yaml") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a rendered file is readable");
            let mut written = String::new();
            for document in serde_norway::Deserializer::from_str(&text) {
                let Ok(mut value) = serde::Deserialize::deserialize(document) else {
                    continue;
                };
                mutate(&mut value);
                written.push_str(
                    "---
",
                );
                written.push_str(
                    &serde_norway::to_string(&value).expect("a manifest serialises back to YAML"),
                );
            }
            std::fs::write(at.join(path.file_name().expect("a named file")), written)
                .expect("a mutant file is written");
        }
        Self(at)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Mutant {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Introduce the three defects wherever this manifest has somewhere to put them.
fn mutate(value: &mut serde_json::Value) {
    // A key nothing declares, inside every rendered configuration document. Prepended rather than
    // appended so it lands before any table header, where TOML would otherwise read it as a member
    // of the last table.
    if value.get("kind").and_then(|kind| kind.as_str()) == Some("ConfigMap")
        && let Some(data) = value.get_mut("data").and_then(|data| data.as_object_mut())
    {
        for held in data.values_mut() {
            if let Some(text) = held.as_str()
                && text.contains(" = ")
            {
                *held = serde_json::Value::String(format!(
                    "parity_unknown_key = 1
{text}"
                ));
            }
        }
    }

    walk_pod_specs(value, &mut |spec| {
        spec.insert(
            "enableServiceLinks".to_owned(),
            serde_json::Value::Bool(true),
        );
        for group in ["initContainers", "containers"] {
            let Some(containers) = spec.get_mut(group).and_then(|held| held.as_array_mut()) else {
                continue;
            };
            for container in containers {
                let Some(container) = container.as_object_mut() else {
                    continue;
                };
                let env = container
                    .entry("env")
                    .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                if let Some(env) = env.as_array_mut() {
                    env.push(serde_json::json!({
                        "name": "PARITY_UNACCOUNTED_VARIABLE", "value": "x"
                    }));
                }
            }
        }
    });
}

/// Every pod spec in one manifest, whichever nesting its workload uses.
fn walk_pod_specs(
    value: &mut serde_json::Value,
    apply: &mut impl FnMut(&mut serde_json::Map<String, serde_json::Value>),
) {
    let is_pod_spec = value
        .as_object()
        .is_some_and(|fields| fields.contains_key("containers"));
    if is_pod_spec {
        if let Some(fields) = value.as_object_mut() {
            apply(fields);
        }
        return;
    }
    match value {
        serde_json::Value::Object(fields) => {
            for held in fields.values_mut() {
                walk_pod_specs(held, apply);
            }
        }
        serde_json::Value::Array(items) => {
            for held in items {
                walk_pod_specs(held, apply);
            }
        }
        _ => {}
    }
}

/// The checkout to compare over, when one was named.
fn tree() -> Option<PathBuf> {
    let named = std::env::var("TERRACE_PARITY_CHARTS").ok()?;
    let root = PathBuf::from(named);
    root.join("charts").is_dir().then_some(root)
}

/// What this crate finds, sorted.
fn rust_side(root: &Path, rendered: &Path) -> Vec<Finding> {
    let checked = helm::check(&root.join("charts"), rendered).unwrap_or_else(|failure| {
        panic!("this crate could not read {}: {failure}", root.display())
    });
    let mut found: Vec<Finding> = checked
        .report
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.at.clone(),
                match entry.finding.level {
                    Level::Error => "error".to_owned(),
                    Level::Warning => "warning".to_owned(),
                },
                entry.finding.message.clone(),
            )
        })
        .collect();
    found.sort();
    found
}

/// What the oracle finds, or [`None`] when it is not there to ask.
fn python_side(root: &Path, rendered: &Path) -> Option<Vec<Finding>> {
    if !root.join(".github/scripts/check-config.py").is_file() {
        return None;
    }
    let adapter = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("parity")
        .join("oracle.py");

    let mut output = None;
    for interpreter in interpreters() {
        let attempt = Command::new(&interpreter)
            .arg(&adapter)
            .arg(root)
            .arg(rendered)
            .output();
        if let Ok(attempt) = attempt {
            if attempt.status.success() {
                output = Some(attempt);
                break;
            }
            panic!(
                "the oracle failed under {interpreter}:\n{}",
                String::from_utf8_lossy(&attempt.stderr)
            );
        }
    }

    let output = output.unwrap_or_else(|| {
        panic!(
            "the Python gate is present at {} and no interpreter could run it. Set \
             TERRACE_PARITY_PYTHON, or unset TERRACE_PARITY_CHARTS to skip.",
            root.display()
        )
    });

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("the oracle did not print JSON: {e}"));
    assert!(
        parsed.get("error").is_none(),
        "the oracle could not run: {}",
        parsed["error"]
    );

    let mut found: Vec<Finding> = parsed["findings"]
        .as_array()
        .expect("the oracle prints a list of findings")
        .iter()
        .map(|entry| {
            (
                entry["where"].as_str().unwrap_or_default().to_owned(),
                entry["level"].as_str().unwrap_or_default().to_owned(),
                entry["message"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    found.sort();
    Some(found)
}

/// The interpreters to try, in order.
///
/// `python3` is the spelling every recipe uses and is a stub that fails on a stock Windows, so the
/// list is a list rather than a constant.
fn interpreters() -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(named) = std::env::var("TERRACE_PARITY_PYTHON") {
        found.push(named);
    }
    found.push("python3".to_owned());
    found.push("python".to_owned());
    found
}

/// The findings one side has and the other does not, counting duplicates.
fn missing_from<'a>(mine: &'a [Finding], theirs: &[Finding]) -> Vec<&'a Finding> {
    let mut remaining: BTreeMap<&Finding, usize> = BTreeMap::new();
    for finding in theirs {
        *remaining.entry(finding).or_default() += 1;
    }
    let mut only = Vec::new();
    for finding in mine {
        match remaining.get_mut(finding) {
            Some(held) if *held > 0 => *held -= 1,
            _ => only.push(finding),
        }
    }
    only
}

/// The differences no recorded decision accounts for.
fn unexplained<'a>(only: &[&'a Finding], side: Side) -> Vec<&'a Finding> {
    only.iter()
        .filter(|(_, _, message)| {
            !DELIBERATE
                .iter()
                .any(|entry| entry.side == side && message.starts_with(entry.opening))
        })
        .copied()
        .collect()
}

/// A list of findings, one per line, for a failure message somebody has to act on.
fn render(findings: &[&Finding]) -> String {
    use std::fmt::Write as _;

    if findings.is_empty() {
        return "  (none)
"
        .to_owned();
    }
    let mut rendered = String::new();
    for (at, level, message) in findings {
        let _ = writeln!(rendered, "  [{level}] {at}: {message}");
    }
    rendered
}
