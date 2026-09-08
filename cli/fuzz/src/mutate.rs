//! Turning a byte string into a document worth checking.
//!
//! A fuzzer handed raw bytes and asked for a contract produces JSON that fails to parse
//! essentially every time, and an oracle that only ever exercises the error path proves the reader
//! rejects garbage — which was never in doubt. The interesting inputs are documents that are
//! *almost* right: a real one with a prefix emptied, a secret given a default, an ignore pattern
//! widened by one character.
//!
//! So the input is read as a small directive language applied to one of the stored conformance
//! cases. The fuzzer mutates directives; the directives mutate a document that was valid to begin
//! with; and what reaches the oracle is a document deep enough for a rule to have an opinion about.
//!
//! Unparseable directives are skipped rather than failed. A mutation engine spends most of its
//! time producing near-misses, and a harness that gave up on the first one would explore nothing.

use serde_json::{Map, Value as Json, json};

/// The stored cases, embedded so an oracle needs no working directory.
///
/// A fuzz target runs from wherever libFuzzer was invoked, and `cargo fuzz` and `cargo test`
/// disagree about where that is. Reading them from disk would make the corpus replay depend on a
/// path that is right in one of the two.
const CASES: &[(&str, &str)] = &[
    (
        "minimal",
        include_str!("../../../spec/v1/conformance/minimal/contract.json"),
    ),
    (
        "full-surface",
        include_str!("../../../spec/v1/conformance/full-surface/contract.json"),
    ),
    (
        "unnameable-key",
        include_str!("../../../spec/v1/conformance/unnameable-key/contract.json"),
    ),
];

/// One stored case as a mutable JSON tree, chosen by name.
///
/// An unrecognised name falls back to the first case rather than failing: the first line of a
/// mutated input is the one a fuzzer corrupts most often, and refusing there would throw away the
/// directives below it.
fn base(name: &str) -> Json {
    let text = CASES
        .iter()
        .find(|(case, _)| *case == name)
        .map_or(CASES[0].1, |(_, text)| *text);
    serde_json::from_str(text).expect("a stored conformance case is valid JSON")
}

/// Apply an input's directives to a stored case.
///
/// The first line names the case; every line after it is one directive. See the module docs for
/// why nothing here refuses.
pub fn document(input: &str) -> Json {
    let mut lines = input.lines();
    let mut value = base(lines.next().unwrap_or("minimal").trim());

    for line in lines {
        apply(&mut value, line.trim_end_matches('\r'));
    }
    value
}

/// One directive.
///
/// Every arm is a rule some oracle has an opinion about. `p=` and `g=` reach the ignore-pattern
/// refusals; `k=<n>:secret=true` reaches the secret-with-a-default one; `V=` and `S=` reach the
/// envelope gate; `L=` reaches the loader registry that decides whether a text read may happen at
/// all.
fn apply(value: &mut Json, line: &str) {
    let Some((kind, rest)) = line.split_once('=') else {
        // `r` is the one directive with no argument.
        if line == "r" {
            set(value, &["schema", "loader"], json!([]));
        }
        return;
    };

    match kind {
        "p" => set(value, &["schema", "dialect", "prefix"], json!(rest)),
        "n" => set(
            value,
            &["schema", "dialect", "nesting_separator"],
            json!(rest),
        ),
        "i" => set(
            value,
            &["schema", "dialect", "indirection_suffix"],
            json!(rest),
        ),
        "L" => set(value, &["producer", "loader"], json!(rest)),
        "V" => {
            if let Ok(version) = rest.parse::<u32>() {
                set(value, &["terrace_contract"], json!(version));
            }
        }
        "S" => {
            if let Ok(version) = rest.parse::<u32>() {
                set(value, &["schema", "schema_version"], json!(version));
            }
        }
        "x" => push(
            value,
            &["external", "env"],
            json!({
                "name": rest,
                "docs": "",
                "values": [],
                "text_form": "text",
                "required": false,
                "secret": false
            }),
        ),
        "g" => push(value, &["external", "ignore"], json!(rest)),
        "u" => set(value, &["external", "unknown"], json!(rest)),
        "K" => push(value, &["schema", "keys"], key(rest)),
        "k" => mutate_key(value, rest),
        _ => {}
    }
}

/// A fresh key at a path, with every field the meta-schema requires.
fn key(path: &str) -> Json {
    json!({
        "path": path,
        "env": null,
        "env_file": null,
        "secrets_file": null,
        "docs": "",
        "ty": "String",
        "values": [],
        "constraint": {"type": "string"},
        "text_form": "text",
        "aliases": [],
        "env_aliases": [],
        "env_file_aliases": [],
        "secrets_file_aliases": [],
        "unreachable": "unnameable",
        "default": null,
        "default_value": null,
        "note": null,
        "required": false,
        "secret": false,
        "reserved": false
    })
}

/// `k=<index>:<field>=<value>` — one field of one key.
fn mutate_key(value: &mut Json, rest: &str) {
    let Some((index, assignment)) = rest.split_once(':') else {
        return;
    };
    let Ok(index) = index.parse::<usize>() else {
        return;
    };
    let Some((field, raw)) = assignment.split_once('=') else {
        return;
    };

    let Some(keys) = value
        .get_mut("schema")
        .and_then(|schema| schema.get_mut("keys"))
        .and_then(Json::as_array_mut)
    else {
        return;
    };
    if keys.is_empty() {
        return;
    }
    // Wrapped rather than bounds-checked: an index past the end is the commonest thing a mutation
    // engine produces, and skipping those would leave most inputs doing nothing at all.
    let wrapped = index % keys.len();
    let Some(Json::Object(target)) = keys.get_mut(wrapped) else {
        return;
    };

    let parsed = match field {
        "secret" | "required" | "reserved" => json!(raw == "true"),
        "env" | "env_file" | "secrets_file" | "default" | "path" | "ty" | "unreachable"
        | "text_form" => {
            if raw == "null" {
                Json::Null
            } else {
                json!(raw)
            }
        }
        // Read as JSON so a fuzzer can reach a number, an array or an object here — the three
        // shapes the TOML literal writer treats differently.
        "default_value" | "constraint" | "text_constraint" => {
            serde_json::from_str(raw).unwrap_or(Json::Null)
        }
        "aliases" => serde_json::from_str(raw).unwrap_or_else(|_| json!([])),
        _ => return,
    };
    target.insert(field.to_owned(), parsed);
}

/// Set a value at a path of object keys, creating nothing.
fn set(value: &mut Json, path: &[&str], new: Json) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut cursor = value;
    for segment in parents {
        let Some(next) = cursor.get_mut(*segment) else {
            return;
        };
        cursor = next;
    }
    if let Json::Object(object) = cursor {
        object.insert((*last).to_owned(), new);
    }
}

/// Append to an array at a path, creating nothing.
fn push(value: &mut Json, path: &[&str], new: Json) {
    let mut cursor = value;
    for segment in path {
        let Some(next) = cursor.get_mut(*segment) else {
            return;
        };
        cursor = next;
    }
    if let Json::Array(array) = cursor {
        array.push(new);
    }
}

/// The document as the bytes a reader would be handed.
///
/// # Panics
/// If a JSON tree built here does not serialise, which would be a defect in this harness rather
/// than a finding about the crate under test.
pub fn render(value: &Json) -> String {
    serde_json::to_string(value).expect("a JSON tree serialises")
}

/// Every stored case's name, for a generator that wants to pick one.
pub fn case_names() -> impl Iterator<Item = &'static str> {
    CASES.iter().map(|(name, _)| *name)
}

/// A stored case, unmutated — the fixed point every oracle checks against.
///
/// # Panics
/// If a stored conformance case is not a JSON object, which the corpus tests would have caught
/// first.
pub fn stored(name: &str) -> Map<String, Json> {
    match base(name) {
        Json::Object(object) => object,
        _ => unreachable!("a contract document is an object"),
    }
}
