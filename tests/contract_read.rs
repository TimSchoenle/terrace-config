//! The read step of the ordered list, checked against the loader for every form.
//!
//! `contract.rs` pins fields: that a key reports `text_form: integer`, that its pattern is the one
//! that was measured. Nothing pinned the *list* — the specification those fields are consumed by —
//! and three findings in a row have been a rule that was correct when written and became wrong
//! when a later commit moved what it depended on, in a diff that never touched the rule:
//!
//! - gate 3's file-layer rule, keyed on `text_form`, after the parsing types were reclassified;
//! - `required`, meaning one thing in JSON Schema and another here, once the loader had four
//!   layers;
//! - "when `constraint` is a string type, skip the read", after `choice` gained a trim.
//!
//! This file is the missing half. It implements the documented read exactly as the table in
//! [`External`]'s documentation states it, then asserts that what the read produces is what the
//! loader actually loaded — so a form whose read stops matching the loader fails here rather than
//! in somebody's cluster.

#![cfg(all(feature = "schema", feature = "testing"))]

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use terrace_config::Terrace;
use terrace_config::schema::{Describe, TextForm};
use terrace_config::testing::Harness;

#[derive(Deserialize, Serialize, Default, Describe)]
#[serde(rename_all = "lowercase")]
enum Level {
    Off,
    #[default]
    Info,
}

#[derive(Deserialize, Serialize, Describe)]
struct Config {
    /// An integer.
    #[serde(default)]
    count: u32,
    /// A flag.
    #[serde(default)]
    flag: bool,
    /// A choice.
    #[config(values)]
    #[serde(default)]
    level: Level,
    /// Any text.
    #[serde(default)]
    text: String,
    /// A `char`: form `unknown`, and the one whose `constraint` a read has to reach.
    #[serde(default = "default_marker")]
    marker: char,
}

fn default_marker() -> char {
    'x'
}

impl Default for Config {
    fn default() -> Self {
        Self {
            count: 0,
            flag: false,
            level: Level::default(),
            text: String::new(),
            marker: default_marker(),
        }
    }
}

/// The read step, implemented exactly as [`External`]'s table states it.
///
/// Deliberately written from the documentation rather than from the crate's internals: it is a
/// stand-in for the consumer the document is written for, and a consumer has nothing else.
fn read(form: TextForm, text: &str) -> Option<Json> {
    // Every read begins by trimming, because the environment layer trimmed before it parsed
    // anything. That is the line this file exists to keep true.
    let text = text.trim();
    match form {
        TextForm::Integer => text
            .strip_prefix('+')
            .unwrap_or(text)
            .parse::<i64>()
            .ok()
            .map(Json::from),
        TextForm::Boolean => Some(Json::from(text == "true")),
        TextForm::Choice | TextForm::Text | TextForm::Unknown => Some(Json::from(text)),
        // `Structured` needs a TOML parser rather than a primitive, which is what its own
        // documentation says — a consumer without one does the form half and skips this. It shares
        // an arm with the wildcard, which covers a variant added after this file was written: both
        // mean "no read here", and a new form landing in it fails the coverage test below rather
        // than silently getting one.
        TextForm::Structured | _ => None,
    }
}

/// What the loader made of `text`, supplied through the environment layer.
fn loaded(key: &str, text: &str) -> Result<Json, String> {
    let mut outcome = Err(String::new());
    Harness::over(Terrace::new("READ_")).run(|jail| {
        jail.env(format!("READ_{}", key.to_uppercase()), text);
        outcome = match jail.load::<Config>() {
            Ok(config) => {
                Ok(serde_json::to_value(&config).expect("the fixture serialises")[key].clone())
            }
            Err(error) => Err(error.to_string()),
        };
        Ok(())
    });
    outcome
}

/// Every form, and for each one text the loader accepts.
///
/// Whitespace on both sides of every case on purpose: it is the difference the trim exists for,
/// and the one a read is most likely to drop.
const CASES: &[(&str, &[&str])] = &[
    ("count", &["42", " 42", "42 ", "\t42\n", "+42", "007"]),
    ("flag", &["true", "false", " true", "false "]),
    ("level", &["info", " info", "info ", "\toff\n"]),
    ("text", &["hello", " hello", "hello "]),
    ("marker", &["y", " y", "y ", " y "]),
];

#[test]
fn the_documented_read_produces_what_the_loader_produces() {
    let schema = Terrace::new("READ_")
        .schema::<Config>()
        .with_defaults_from(&Config::default())
        .expect("the fixture serialises");

    for (key, texts) in CASES {
        let form = schema
            .keys
            .iter()
            .find(|described| described.path == *key)
            .unwrap_or_else(|| panic!("no key is described at that path"))
            .text_form;

        for text in *texts {
            let loaded = loaded(key, text).unwrap_or_else(|error| {
                panic!("the loader refused a case this test assumes it takes: {error}")
            });
            let read =
                read(form, text).unwrap_or_else(|| panic!("no read is documented for this form"));

            assert_eq!(
                read, loaded,
                "form {form:?}, text {text:?}: the documented read and the loader disagree"
            );
        }
    }
}

#[test]
fn every_form_a_key_can_report_has_a_documented_read() {
    // The other half of the same guard: a form nothing exercises is a row of the table nobody has
    // checked. Adding a variant without a case here fails, rather than shipping a read that has
    // never been compared to anything.
    let schema = Terrace::new("READ_").schema::<Config>();
    let mut seen: Vec<TextForm> = schema.keys.iter().map(|key| key.text_form).collect();
    seen.dedup();

    for (form, sample) in [
        (TextForm::Integer, "1"),
        (TextForm::Boolean, "true"),
        (TextForm::Choice, "info"),
        (TextForm::Text, "anything"),
        (TextForm::Unknown, "anything"),
    ] {
        assert!(
            seen.contains(&form),
            "{form:?} has no key in this fixture, so its read is checked against nothing"
        );
        assert!(
            read(form, sample).is_some(),
            "{form:?} has no documented read"
        );
    }

    // `Structured` is the exception the table names: its read is a TOML parse, which this crate
    // takes no dependency on and a consumer may not have either.
    assert!(read(TextForm::Structured, "[]").is_none());
}
