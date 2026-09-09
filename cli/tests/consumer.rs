//! The documents a consumer has to survive, and the properties each one pins.
//!
//! `spec/v1/conformance/` held three cases, all of them things a producer *emits*. These are the
//! other half: documents a consumer must read correctly, none of which the Rust producer can write.
//! A foreign loader, a schema version below the current one, spellings that do not follow from the
//! dialect, and type names from another language.
//!
//! They are hand-written rather than blessed, and that distinction is load-bearing. A producer case
//! has exactly one author — the reference implementation's `TERRACE_SPEC_BLESS` run — and carries a
//! `rendered/` directory of goldens. A consumer case has no producer here that could author it, so
//! it carries no goldens, and what holds it is this file: assertions about what a *reader* must do.
//!
//! The cheapest tests in the suite, and the ones that catch the multi-producer integration breaking
//! months before there is a second producer's document to vendor.

#![cfg(feature = "k8s")]

use std::path::{Path, PathBuf};

use serde_json::{Value as Json, json};
use terrace_contract::gate::{Relaxed, check_container};
use terrace_contract::union::{Union, union_contracts};
use terrace_contract::value::{Range, form, range, reads_for};
use terrace_contract::{Contract, Tier, conform, validate};

/// The cases a producer here can emit, which carry blessed renderings.
const PRODUCER_CASES: &[&str] = &["minimal", "full-surface", "unnameable-key"];

/// The cases only a consumer meets, which carry none and are held by this file.
const CONSUMER_CASES: &[&str] = &[
    "foreign-loader",
    "foreign-ty",
    "schema-version-1",
    "tier-1-spellings",
];

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate has a parent directory")
        .join("spec")
        .join("v1")
        .join("conformance")
}

fn text(case: &str) -> String {
    let path = conformance().join(case).join("contract.json");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()))
}

fn contract(case: &str) -> Contract {
    Contract::from_json(&text(case))
        .unwrap_or_else(|e| panic!("{case} is not a readable contract: {e}"))
}

fn union(case: &str) -> Union {
    let document: Json = serde_json::from_str(&text(case)).expect("the case is JSON");
    union_contracts(&[(case.to_owned(), document)])
        .unwrap_or_else(|e| panic!("{case} does not merge with itself: {e}"))
}

#[test]
fn every_case_in_the_corpus_is_claimed_by_exactly_one_list() {
    // The guard that keeps this file honest. A case added to the tree and listed nowhere is a case
    // nothing checks, which looks from the outside exactly like a case that passes.
    let mut found: Vec<String> = std::fs::read_dir(conformance())
        .expect("the conformance corpus is there")
        .map(|entry| entry.expect("a case directory"))
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();

    let mut claimed: Vec<String> = PRODUCER_CASES
        .iter()
        .chain(CONSUMER_CASES.iter())
        .map(|case| (*case).to_owned())
        .collect();
    claimed.sort();

    assert_eq!(
        found, claimed,
        "every directory under spec/v1/conformance/ must be listed as a producer case or a \
         consumer case"
    );
}

#[test]
fn a_producer_case_carries_renderings_and_a_consumer_case_does_not() {
    // The absence is the statement: nothing here authored these bytes, so nothing here may bless a
    // rendering of them. Asserted rather than left implicit, because an empty `rendered/` appearing
    // one day would otherwise silently mean "the goldens are fine".
    for case in PRODUCER_CASES {
        assert!(
            conformance().join(case).join("rendered").is_dir(),
            "{case} is a producer case and must carry the goldens its own implementation blessed"
        );
    }
    for case in CONSUMER_CASES {
        assert!(
            !conformance().join(case).join("rendered").exists(),
            "{case} has no producer here that could author a rendering of it, so it must carry no \
             goldens — two renderers that can each rewrite the expectation prove nothing"
        );
    }
}

#[test]
fn every_consumer_case_is_a_document_this_build_reads_and_the_spec_describes() {
    for case in CONSUMER_CASES {
        let failures = validate::validate(&text(case)).expect("the case is JSON");
        assert!(
            failures.is_empty(),
            "{case} does not satisfy spec/v1/contract.schema.json: {failures:?}"
        );
        let violations = conform::conform(&contract(case), Tier::Document);
        assert!(
            violations.is_empty(),
            "{case} does not conform at tier 1: {violations:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// foreign-loader — step 2 skipped and reported, not silently performed
// ---------------------------------------------------------------------------------------------

#[test]
fn a_loader_with_no_measured_row_gets_no_range_check() {
    let merged = union("foreign-loader");
    assert_eq!(merged.loader_name, "spring-boot");
    assert!(
        reads_for(&merged.loader_name).is_none(),
        "spring-boot's reads have not been measured, and an unmeasured row is worse than no row"
    );

    let port = merged
        .keys
        .get("server.port")
        .expect("the case declares server.port");
    // 99999 is a well-formed integer and is above a u16's maximum, which is exactly the value only
    // the range step can catch — and exactly the one that must not be caught here.
    assert_eq!(
        form(port.entry(), "99999").expect("the form is known"),
        None,
        "the form step reads `text_constraint`, which the producer published about its own reads"
    );
    let found = range(port.entry(), &merged.loader_name, "99999").expect("the form is known");
    let Range::NotChecked(reason) = found else {
        panic!("the range step must be skipped for a loader with no measured row: {found:?}");
    };
    assert!(reason.starts_with("range not checked:"), "{reason}");
}

#[test]
fn the_form_step_still_refuses_text_no_loader_could_read() {
    let merged = union("foreign-loader");
    let port = merged.keys.get("server.port").expect("server.port");
    let failure = form(port.entry(), "http")
        .expect("the form is known")
        .expect("`http` does not match the published pattern");
    assert!(failure.contains("does not match"), "{failure}");
}

#[test]
fn a_gate_over_a_foreign_loader_reports_the_skip_rather_than_a_failure() {
    let merged = union("foreign-loader");
    let container = json!({
        "name": "app",
        "env": [{"name": "FOREIGN_SERVER__PORT", "value": "99999"}],
    });
    let findings = check_container(&[], &json!({}), &container, &merged, Relaxed::default())
        .expect("the contract is readable");
    assert_eq!(
        findings,
        Vec::new(),
        "a bound measured against another loader's reads must not stop this deployment"
    );
}

// ---------------------------------------------------------------------------------------------
// schema-version-1 — degrade rather than assume, narrow rather than refuse
// ---------------------------------------------------------------------------------------------

#[test]
fn a_version_one_document_publishes_no_element_schema_and_that_is_an_answer() {
    let merged = union("schema-version-1");
    assert_eq!(
        contract("schema-version-1").schema.schema_version,
        1,
        "the case exists to be below the current version"
    );

    for path in ["peers", "methods"] {
        let held = merged.keys.get(path).expect("the case declares it");
        let constraint = held
            .fields
            .get("constraint")
            .and_then(Json::as_object)
            .expect("a container key still says what it is");
        assert!(
            !constraint.contains_key("items") && !constraint.contains_key("additionalProperties"),
            "{path} must say nothing about its elements, so a reader has to degrade rather than \
             read past the end of what the producer said"
        );
    }
}

#[test]
fn a_version_below_the_current_one_is_not_reported_as_ahead() {
    // The check is one-sided on purpose: a *higher* version may carry a keyword this build would
    // walk past, and a lower one nests nowhere and is read exactly as it always was.
    assert_eq!(contract("schema-version-1").schema_version_ahead(), None);
}

#[test]
fn a_structured_key_is_read_before_anything_is_said_about_it() {
    // Not "no range step". A container still has to be *read* — the read is what says `a,b` is not
    // a list — and whether it is one belongs to the loader: figment wants a TOML literal, and a
    // binder that splits on commas reads the same text as two items.
    let merged = union("schema-version-1");
    let peers = merged.keys.get("peers").expect("peers");
    assert_eq!(
        range(peers.entry(), &merged.loader_name, "{a = 1}").expect("the form is known"),
        Range::Ok
    );
    let found = range(peers.entry(), &merged.loader_name, "a,b").expect("the form is known");
    let Range::Wrong(failure) = found else {
        panic!("a list without its brackets is the deployment this form exists to name: {found:?}");
    };
    assert!(failure.contains("brackets"), "{failure}");

    // And a version-1 document says nothing about the elements, so nothing is asserted about them:
    // the container reads, and the reader degrades rather than inventing an element schema.
    assert_eq!(
        range(peers.entry(), &merged.loader_name, "{a = 1, b = \"two\"}")
            .expect("the form is known"),
        Range::Ok
    );
}

// ---------------------------------------------------------------------------------------------
// tier-1-spellings — derive nothing the document states
// ---------------------------------------------------------------------------------------------

#[test]
fn a_spelling_that_does_not_follow_from_the_dialect_is_still_the_spelling() {
    let merged = union("tier-1-spellings");
    assert_eq!(merged.nesting_separator(), "__");

    // `RELAXED_SERVERPORT` is what the binder reads. Deriving it from `server.port` and `__` would
    // produce `RELAXED_SERVER__PORT`, which nothing reads.
    let derived = format!("{}SERVER__PORT", merged.prefix());
    assert!(
        merged.key_by("env", &derived).is_none(),
        "no key is spelled {derived}, and a rule that derived one would find a key that is not there"
    );
    assert_eq!(
        merged
            .key_by("env", "RELAXED_SERVERPORT")
            .map(|key| key.name.as_str()),
        Some("server.port")
    );
    assert_eq!(
        merged
            .key_by("env", "RELAXED_DATASOURCE_URL")
            .map(|key| key.name.as_str()),
        Some("data.source.url")
    );
}

#[test]
fn a_key_that_names_no_variable_says_why_rather_than_leaving_a_gap() {
    let merged = union("tier-1-spellings");
    let ttl = merged.keys.get("cache.ttl").expect("cache.ttl");
    assert_eq!(ttl.fields.get("env"), Some(&Json::Null));
    assert_eq!(
        ttl.text("unreachable"),
        Some("unnameable"),
        "a consumer meeting a bare null must be able to tell 'the environment cannot reach this' \
         from 'skip this key'"
    );
}

#[test]
fn the_one_rule_that_reads_the_separator_can_only_turn_a_finding_into_silence() {
    let merged = union("tier-1-spellings");
    // No key here is `structured`, so the dynamic-map legitimiser has nothing to legitimise — and
    // a variable inside the namespace that spells nothing is still reported.
    assert!(
        merged
            .container_of("env", "RELAXED_SERVERPORT__ANYTHING")
            .is_none()
    );

    let container = json!({"name": "app", "env": [{"name": "RELAXED_SERVER__PORT", "value": "1"}]});
    let findings = check_container(&[], &json!({}), &container, &merged, Relaxed::default())
        .expect("the contract is readable");
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0]
            .message
            .contains("matches no key in the contract"),
        "{findings:?}"
    );
}

#[test]
fn only_the_rules_that_admit_to_a_tier_2_assumption_read_the_separator() {
    // The mechanical half of "derive nothing the document states", and the reason it is a test
    // rather than a convention: a new rule that reaches for `dialect.nesting_separator` to work out
    // what a key is called has silently assumed tier 2 of every producer, and nothing else in the
    // suite would notice. A grep is a blunt instrument and it is the right one — the property is
    // about which files mention the field at all.
    const ALLOWED: &[&str] = &[
        // Declares the field. Reading a document is not assuming anything about it.
        "document.rs",
        // Tier 2 *is* the derivation, and checking whether a producer meets it is the one place
        // deriving a spelling is the whole point.
        "conform.rs",
        // `Union::container_of`, the dynamic-map legitimiser, which says so in its own
        // documentation and can only turn a finding into silence.
        "union.rs",
    ];

    let mut found: Vec<String> = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut |path, body| {
            // Test fixtures spell a whole dialect and assume nothing; the rules are what matter.
            let rules = body.split("#[cfg(test)]").next().unwrap_or(body);
            if rules.contains("nesting_separator") {
                found.push(
                    path.file_name()
                        .expect("a named file")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        },
    );
    found.sort();
    found.dedup();

    let mut allowed: Vec<String> = ALLOWED.iter().map(|name| (*name).to_owned()).collect();
    allowed.sort();
    assert_eq!(
        found, allowed,
        "a rule outside the three that admit to it is reading `dialect.nesting_separator`. Every          other rule must read the spellings the document published: a producer that hands naming          to a binder with its own relaxed-binding rules does not reach tier 2, and a rule that          assumes it silently is wrong about that producer."
    );
}

/// Every `.rs` file under a directory, with its body.
fn walk(at: &Path, apply: &mut impl FnMut(&Path, &str)) {
    for entry in std::fs::read_dir(at).expect("the source tree is readable") {
        let path = entry.expect("a source entry").path();
        if path.is_dir() {
            walk(&path, apply);
        } else if path.extension().and_then(std::ffi::OsStr::to_str) == Some("rs") {
            let body = std::fs::read_to_string(&path).expect("a source file is readable");
            apply(&path, &body);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// foreign-ty — a type vocabulary from another language changes no behaviour
// ---------------------------------------------------------------------------------------------

#[test]
fn removing_every_ty_changes_nothing_a_rule_decides() {
    // The property stated so it can be checked rather than remembered. `ty` is token text in the
    // producer's language, and the moment one `match` arm spells a type name the toolchain is
    // single-producer again.
    let with = union("foreign-ty");
    let without = {
        let mut document: Json = serde_json::from_str(&text("foreign-ty")).expect("JSON");
        for key in document["schema"]["keys"]
            .as_array_mut()
            .expect("the case has keys")
        {
            key.as_object_mut()
                .expect("a key is an object")
                .remove("ty");
        }
        union_contracts(&[("foreign-ty".to_owned(), document)]).expect("it merges with itself")
    };

    let container = json!({
        "name": "app",
        "env": [
            {"name": "FOREIGNTY_CACHE__TTL", "value": "-1"},
            {"name": "FOREIGNTY_METHODS", "value": "get,post"},
            {"name": "FOREIGNTY_TOKEN", "value": "x"},
            {"name": "FOREIGNTY_SOMETHING", "value": "x"},
        ],
    });
    let mine = check_container(&[], &json!({}), &container, &with, Relaxed::default())
        .expect("the contract is readable");
    let theirs = check_container(&[], &json!({}), &container, &without, Relaxed::default())
        .expect("the contract is readable");

    assert!(
        !mine.is_empty(),
        "the fixture must actually reach some rules"
    );
    assert_eq!(
        mine, theirs,
        "a rule read `ty` and decided something with it, which makes this toolchain single-producer"
    );
}

#[test]
fn a_foreign_type_name_is_printed_where_a_message_shows_one_and_nowhere_else() {
    // The one legitimate use: a reader's aid in the message that explains why a file cannot supply
    // a key. It prints whatever is there and switches on `text_form`.
    let merged = union("foreign-ty");
    let container = json!({
        "name": "app",
        "env": [{"name": "FOREIGNTY_CACHE__TTL_FILE", "value": "/secrets/ttl"}],
        "volumeMounts": [{"name": "v", "mountPath": "/secrets"}],
    });
    let findings = check_container(&[], &json!({}), &container, &merged, Relaxed::default())
        .expect("the contract is readable");
    let named = findings
        .iter()
        .find(|finding| finding.message.contains("cannot be supplied by a file"))
        .expect("an integer key cannot be file-supplied");
    assert!(
        named.message.contains("(type 'java.time.Duration')"),
        "{}",
        named.message
    );
    assert!(
        !named.message.contains("Rust"),
        "the producer's own name for the type is not a Rust type name: {}",
        named.message
    );
}
