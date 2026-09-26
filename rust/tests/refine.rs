//! Refinements: constraints a type cannot state, supplied when the schema is built.
//!
//! Every rule `Schema::refine` documents is pinned here, because each one is a way the published
//! contract could claim a check it does not carry — or carry one that contradicts the rest of the
//! key. The failure these exist to prevent is concrete: a host that refuses to start without an
//! `imprint` document, a contract publishing `{}` as a valid default for that map, and a chart that
//! renders exactly that, passes every gate, and fails at boot.

#![cfg(feature = "schema")]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use terrace_config::Terrace;
use terrace_config::schema::{
    App, Describe, Key, LATEST_SCHEMA_VERSION, Refine, Refinement, SCHEMA_VERSION, Schema,
};

#[derive(Deserialize, Serialize, Default, Describe)]
struct Host {
    /// Port the server binds.
    #[serde(default)]
    port: u16,
    #[config(nested)]
    legal: Legal,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Legal {
    /// Legal documents, by the name the site links them under.
    #[config(element)]
    #[serde(default, alias = "docs")]
    documents: BTreeMap<String, LegalDocument>,
    /// Footer links, by label. A map whose element the walk reads from the type alone.
    #[serde(default)]
    links: BTreeMap<String, String>,
    /// Shown above every document.
    #[serde(default)]
    title: String,
    /// Paths served verbatim.
    #[serde(default)]
    assets: Vec<String>,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct LegalDocument {
    /// Heading of the page.
    title: String,
    /// Markdown body.
    #[serde(default)]
    body: String,
}

const DOCUMENTS: &str = "legal.documents";

fn described() -> Schema {
    Terrace::new("PORTFOLIO_").schema::<Host>()
}

/// Described and defaulted from an empty `Host`, which is what `#[serde(default)]` loads to.
fn defaulted() -> Schema {
    described()
        .with_defaults_from(&Host::default())
        .expect("the default config serialises")
}

fn imprint_and_privacy() -> Refinement {
    Refinement::required_entries(["privacy", "imprint"])
}

fn key<'a>(schema: &'a Schema, path: &str) -> &'a Key {
    schema
        .keys
        .iter()
        .find(|key| key.path == path)
        .unwrap_or_else(|| panic!("`{path}` is described"))
}

fn constraint(schema: &Schema, path: &str) -> Json {
    key(schema, path)
        .constraint
        .clone()
        .unwrap_or_else(|| panic!("`{path}` has a constraint"))
}

fn refusal(result: Result<Schema, terrace_config::Error>) -> String {
    match result {
        Ok(_) => panic!("the refinement was accepted"),
        Err(error) => error.to_string(),
    }
}

/// A host whose legal documents must include both pages, and whose default already does.
fn with_both_documents() -> Host {
    let mut host = Host::default();
    for name in ["imprint", "privacy"] {
        host.legal.documents.insert(
            name.to_owned(),
            LegalDocument {
                title: name.to_owned(),
                body: String::new(),
            },
        );
    }
    host
}

// ---- what a refinement publishes ----

#[test]
fn required_entries_land_in_the_constraint_sorted_and_the_map_stays_open() {
    let schema = described()
        .refine(DOCUMENTS, imprint_and_privacy())
        .expect("a map key accepts required entries");

    let refined = constraint(&schema, DOCUMENTS);
    assert_eq!(refined["required"], json!(["imprint", "privacy"]));
    assert_eq!(refined["type"], json!("object"));
    // What the type stated is untouched: the element is still described, and still open.
    assert_eq!(
        refined["additionalProperties"]["properties"]["title"]["type"],
        json!("string")
    );
}

#[test]
fn a_default_the_refinement_rejects_makes_the_key_required_with_no_default() {
    let before = defaulted();
    assert!(!key(&before, DOCUMENTS).required);
    assert!(matches!(
        &key(&before, DOCUMENTS).default_value,
        Some(figment::value::Value::Dict(_, entries)) if entries.is_empty()
    ));

    let refined = before
        .refine(DOCUMENTS, imprint_and_privacy())
        .expect("the refinement applies");
    let documents = key(&refined, DOCUMENTS);
    assert!(
        documents.required,
        "`{{}}` supplies nothing the image accepts"
    );
    assert_eq!(documents.default, None);
    assert_eq!(documents.default_value, None);
}

#[test]
fn a_default_holding_every_entry_stays_a_default() {
    let refined = described()
        .with_defaults_from(&with_both_documents())
        .expect("the default config serialises")
        .refine(DOCUMENTS, imprint_and_privacy())
        .expect("the refinement applies");
    let documents = key(&refined, DOCUMENTS);
    assert!(!documents.required);
    assert!(documents.default_value.is_some());
}

#[test]
fn a_key_with_no_default_keeps_its_required_flag() {
    // Nothing observed, so nothing to contradict: whether absence is acceptable is the type's
    // statement, and the refinement only says what a present value must hold.
    let refined = described()
        .refine(DOCUMENTS, imprint_and_privacy())
        .expect("the refinement applies");
    assert!(!key(&refined, DOCUMENTS).required);
}

#[test]
fn refining_unions_with_what_is_there_and_is_idempotent() {
    let once = described()
        .refine(DOCUMENTS, Refinement::required_entries(["privacy"]))
        .and_then(|schema| schema.refine(DOCUMENTS, imprint_and_privacy()))
        .expect("both refinements apply");
    assert_eq!(
        constraint(&once, DOCUMENTS)["required"],
        json!(["imprint", "privacy"])
    );

    let twice = once
        .clone()
        .refine(DOCUMENTS, imprint_and_privacy())
        .expect("the refinement applies again");
    assert_eq!(once, twice);

    // Narrower than what is there removes nothing: a refinement never loosens.
    let narrower = twice
        .refine(DOCUMENTS, Refinement::required_entries(["imprint"]))
        .expect("the refinement applies");
    assert_eq!(once, narrower);
}

#[test]
fn refining_and_observing_defaults_commute() {
    for host in [Host::default(), with_both_documents()] {
        let refine_first = described()
            .refine(DOCUMENTS, imprint_and_privacy())
            .expect("the refinement applies")
            .with_defaults_from(&host)
            .expect("the default config serialises");
        let defaults_first = described()
            .with_defaults_from(&host)
            .expect("the default config serialises")
            .refine(DOCUMENTS, imprint_and_privacy())
            .expect("the refinement applies");
        assert_eq!(refine_first, defaults_first);
    }
}

#[test]
fn an_empty_set_changes_nothing_but_is_still_checked() {
    let empty = Refinement::required_entries(Vec::<String>::new());
    assert_eq!(
        defaulted()
            .refine(DOCUMENTS, empty.clone())
            .expect("an empty set is accepted"),
        defaulted()
    );

    // Skipping the checks for an empty set would let a typo through exactly when a library
    // happened to compute no entries.
    assert!(
        refusal(described().refine("legal.documnets", empty.clone())).contains("legal.documnets")
    );
    assert!(refusal(described().refine("legal.title", empty)).contains("is not a map"));
}

// ---- what a refinement refuses ----

#[test]
fn an_unknown_path_is_an_error_naming_it() {
    let message = refusal(described().refine("legal.imprint", imprint_and_privacy()));
    assert!(
        message.contains("`legal.imprint` is not a key"),
        "{message}"
    );

    let alias = refusal(described().refine("legal.docs", imprint_and_privacy()));
    assert!(alias.contains("alias of `legal.documents`"), "{alias}");

    let table = refusal(described().refine("legal", imprint_and_privacy()));
    assert!(table.contains("It is a table"), "{table}");
}

#[test]
fn a_key_that_is_not_a_map_is_refused() {
    for path in ["legal.title", "legal.assets", "port"] {
        let message = refusal(described().refine(path, imprint_and_privacy()));
        assert!(
            message.contains(&format!("`{path}` is not a map")),
            "{message}"
        );
    }

    // A map closed to every entry can never hold a required one.
    let mut closed = described();
    closed
        .keys
        .iter_mut()
        .find(|key| key.path == "legal.links")
        .expect("the key is described")
        .constraint = Some(json!({"type": "object", "additionalProperties": false}));
    assert!(refusal(closed.refine("legal.links", imprint_and_privacy())).contains("is not a map"));

    // A key with nothing published says nothing about being a map.
    let mut bare = described();
    bare.keys
        .iter_mut()
        .find(|key| key.path == "legal.links")
        .expect("the key is described")
        .constraint = None;
    assert!(
        refusal(bare.refine("legal.links", imprint_and_privacy()))
            .contains("publishes no constraint")
    );
}

#[test]
fn a_map_whose_element_the_walk_reads_from_the_type_is_refinable_too() {
    let refined = described()
        .refine("legal.links", Refinement::required_entries(["home"]))
        .expect("a map of strings is a map");
    assert_eq!(
        constraint(&refined, "legal.links")["required"],
        json!(["home"])
    );
    assert_eq!(
        constraint(&refined, "legal.links")["additionalProperties"],
        json!({"type": "string"})
    );
}

#[test]
fn an_entry_name_no_layer_can_spell_is_refused_with_the_reason() {
    for (entry, reason) in [
        ("Imprint", "reads that as `legal.documents.imprint`"),
        ("a.b", "reads `.` as nesting"),
        ("", "an empty name"),
        ("terms__v2", "reads that as `legal.documents.terms.v2`"),
        ("imprint_file", "is read as the `_FILE` indirection"),
    ] {
        let message = refusal(described().refine(DOCUMENTS, Refinement::required_entries([entry])));
        assert!(
            message.contains(&format!(
                "`{entry}` cannot be a required entry of `legal.documents`"
            )),
            "{message}"
        );
        assert!(message.contains(reason), "`{entry}`: {message}");
    }
}

// ---- a library's refinements, mounted by the host ----

struct LegalPages;

impl Refine for LegalPages {
    fn refinements(&self) -> Vec<(String, Refinement)> {
        vec![("documents".to_owned(), imprint_and_privacy())]
    }
}

struct WholeMap;

impl Refine for WholeMap {
    fn refinements(&self) -> Vec<(String, Refinement)> {
        vec![(String::new(), imprint_and_privacy())]
    }
}

#[test]
fn a_library_refines_relative_to_where_the_host_mounts_it() {
    let direct = defaulted()
        .refine(DOCUMENTS, imprint_and_privacy())
        .expect("the refinement applies");

    let mounted = defaulted()
        .refine_with("legal", &LegalPages)
        .expect("the library's paths resolve under its mount");
    assert_eq!(mounted, direct);

    let whole = defaulted()
        .refine_with(DOCUMENTS, &WholeMap)
        .expect("an empty relative path is the mount itself");
    assert_eq!(whole, direct);

    let at_root = Terrace::new("PORTFOLIO_")
        .schema::<Legal>()
        .refine_with("", &LegalPages)
        .expect("an empty mount is the root");
    assert_eq!(
        constraint(&at_root, "documents")["required"],
        json!(["imprint", "privacy"])
    );

    // The full path is in the error, so both halves of it are visible to whoever reads it.
    let message = refusal(defaulted().refine_with("site", &LegalPages));
    assert!(message.contains("`site.documents`"), "{message}");
}

// ---- the renderings ----

fn document_validator(schema: &str) -> jsonschema::Validator {
    let schema: Json = serde_json::from_str(schema).expect("the rendering is JSON");
    jsonschema::validator_for(&schema).expect("the rendering is a valid JSON Schema")
}

#[test]
fn the_json_schema_rejects_a_map_missing_an_entry_and_keeps_it_open() {
    let refined = defaulted()
        .refine(DOCUMENTS, imprint_and_privacy())
        .expect("the refinement applies");
    let validator = document_validator(&refined.to_json_schema().expect("it renders"));

    let page = json!({"title": "x"});
    assert!(!validator.is_valid(&json!({"legal": {"documents": {}}})));
    assert!(!validator.is_valid(&json!({"legal": {"documents": {"imprint": page}}})));
    assert!(
        validator.is_valid(&json!({"legal": {"documents": {"imprint": page, "privacy": page}}}))
    );
    assert!(validator.is_valid(
        &json!({"legal": {"documents": {"imprint": page, "privacy": page, "terms": page}}})
    ));
}

#[test]
fn a_key_made_required_makes_its_tables_required_exactly_as_a_derived_one_does() {
    let refined = defaulted()
        .refine(DOCUMENTS, imprint_and_privacy())
        .expect("the refinement applies");
    let rendered: Json =
        serde_json::from_str(&refined.to_json_schema().expect("it renders")).expect("JSON");

    assert_eq!(rendered["required"], json!(["legal"]));
    let legal = &rendered["properties"]["legal"];
    // An alias makes a required key a choice of spellings rather than one name — the same shape
    // `json_schema` gives a derive-required key with an alias.
    assert_eq!(
        legal["allOf"],
        json!([{"anyOf": [{"required": ["documents"]}, {"required": ["docs"]}]}])
    );
    let validator = document_validator(&refined.to_json_schema().expect("it renders"));
    assert!(
        !validator.is_valid(&json!({})),
        "the table holding a required key is required"
    );

    // The contract's rendering keeps `require_present` off, and the entries still travel inside
    // the key's own schema: a document that carries the map is held to them.
    let contract = refined
        .into_contract(App::new("portfolio"))
        .build()
        .expect("a refined schema builds");
    let property = &contract.json_schema["properties"]["legal"]["properties"]["documents"];
    assert_eq!(property["required"], json!(["imprint", "privacy"]));
    assert!(contract.json_schema.get("required").is_none());
}

#[test]
fn the_contract_publishes_the_refined_key() {
    let contract = defaulted()
        .refine_with("legal", &LegalPages)
        .expect("the refinement applies")
        .into_contract(App::new("portfolio"))
        .build()
        .expect("a refined schema builds");
    let document: Json =
        serde_json::from_str(&contract.to_json().expect("it serialises")).expect("JSON");
    let documents = document["schema"]["keys"]
        .as_array()
        .expect("keys")
        .iter()
        .find(|key| key["path"] == DOCUMENTS)
        .expect("the key is published");

    assert_eq!(documents["required"], json!(true));
    assert_eq!(documents["default"], Json::Null);
    assert_eq!(documents["default_value"], Json::Null);
    assert_eq!(
        documents["constraint"]["required"],
        json!(["imprint", "privacy"])
    );
}

#[test]
fn markdown_and_the_toml_example_say_what_the_map_must_contain() {
    let refined = defaulted()
        .refine(DOCUMENTS, imprint_and_privacy())
        .expect("the refinement applies");

    let markdown = refined.to_markdown();
    let row = markdown
        .lines()
        .find(|line| line.starts_with("| `legal.documents`"))
        .expect("the key has a row");
    assert!(
        row.contains("`BTreeMap<String, LegalDocument>`, must contain: `imprint`, `privacy`"),
        "{row}"
    );
    assert!(row.contains("required"), "{row}");

    let example = refined.to_toml_example();
    assert!(
        example.contains("# Must contain: imprint, privacy\n"),
        "{example}"
    );
    assert!(
        example.contains("# Required: nothing loads until this key is supplied.\ndocuments = {}"),
        "{example}"
    );
}

// ---- entry names ----

/// `Slug::from_str` in the legal-pages library that asked for this: one to 64 ASCII bytes, the first
/// a lower-case letter or digit, the rest those or `_` and `-`.
const SLUG: &str = "^[a-z0-9][a-z0-9_-]{0,63}$";

fn slugs() -> Refinement {
    Refinement::entry_names(SLUG)
}

/// A host whose default names a document the pattern rejects.
fn with_a_capitalised_document() -> Host {
    let mut host = Host::default();
    host.legal.documents.insert(
        "Terms".to_owned(),
        LegalDocument {
            title: "Terms".to_owned(),
            body: String::new(),
        },
    );
    host
}

#[test]
fn an_entry_name_pattern_lands_in_property_names_and_raises_the_version() {
    let unrefined = described();
    assert_eq!(unrefined.schema_version, SCHEMA_VERSION);

    let refined = unrefined
        .refine(DOCUMENTS, slugs())
        .expect("a map key accepts an entry-name pattern");
    assert_eq!(
        constraint(&refined, DOCUMENTS)["propertyNames"],
        json!({"pattern": SLUG})
    );
    assert_eq!(refined.schema_version, LATEST_SCHEMA_VERSION);
    // The element the type described is untouched.
    assert_eq!(
        constraint(&refined, DOCUMENTS)["additionalProperties"]["properties"]["title"]["type"],
        json!("string")
    );

    // Required entries alone are version 2's vocabulary, and do not move it.
    let entries_only = described()
        .refine(DOCUMENTS, imprint_and_privacy())
        .expect("the refinement applies");
    assert_eq!(entries_only.schema_version, SCHEMA_VERSION);
}

#[test]
fn a_pattern_outside_the_portable_subset_is_refused_naming_the_construct() {
    for (pattern, reason) in [
        (r"^\S+$", r"`\S` names a different set of characters"),
        ("^a.b$", "`.` excludes a different set of line terminators"),
        ("^[a-z]+?$", "cannot itself be repeated or made lazy"),
        ("(?=a)", "lookaround"),
        ("", "an empty pattern"),
    ] {
        let message = refusal(described().refine(DOCUMENTS, Refinement::entry_names(pattern)));
        assert!(
            message.contains("cannot be the entry-name pattern of `legal.documents`"),
            "{message}"
        );
        assert!(message.contains(reason), "{pattern:?}: {message}");
        assert!(message.contains("Portable patterns"), "{message}");
    }
}

#[test]
fn an_entry_name_pattern_needs_a_map_too() {
    for path in ["legal.title", "legal.assets", "port"] {
        let message = refusal(described().refine(path, slugs()));
        assert!(
            message.contains(&format!("`{path}` is not a map")),
            "{message}"
        );
    }
}

#[test]
fn one_pattern_per_key_and_the_same_one_twice_changes_nothing() {
    let once = described().refine(DOCUMENTS, slugs()).expect("applies");
    let twice = once
        .clone()
        .refine(DOCUMENTS, slugs())
        .expect("applies again");
    assert_eq!(once, twice);

    let message = refusal(once.refine(DOCUMENTS, Refinement::entry_names("^[a-z]+$")));
    assert!(
        message.contains("already holds its entry names to"),
        "{message}"
    );
    assert!(message.contains("Publish one pattern"), "{message}");
}

#[test]
fn a_required_entry_the_pattern_rejects_is_refused_in_either_order() {
    let digits_only = Refinement::entry_names("^[a-z]+$");
    let entries = Refinement::required_entries(["imprint", "privacy2"]);

    let pattern_first = refusal(
        described()
            .refine(DOCUMENTS, digits_only.clone())
            .and_then(|schema| schema.refine(DOCUMENTS, entries.clone())),
    );
    assert!(
        pattern_first.contains("`privacy2` cannot be a required entry of `legal.documents`"),
        "{pattern_first}"
    );
    assert!(pattern_first.contains("does not match"), "{pattern_first}");

    let entries_first = refusal(
        described()
            .refine(DOCUMENTS, entries)
            .and_then(|schema| schema.refine(DOCUMENTS, digits_only)),
    );
    assert!(
        entries_first.contains("requires the entry `privacy2`"),
        "{entries_first}"
    );

    // Entries the pattern admits are fine, whichever arrived first.
    let both = described()
        .refine(DOCUMENTS, imprint_and_privacy())
        .and_then(|schema| schema.refine(DOCUMENTS, slugs()))
        .expect("every required entry is a slug");
    assert_eq!(
        constraint(&both, DOCUMENTS)["required"],
        json!(["imprint", "privacy"])
    );
}

#[test]
fn a_default_naming_an_entry_the_pattern_rejects_is_not_a_default_in_either_order() {
    let refine_first = described()
        .refine(DOCUMENTS, slugs())
        .expect("applies")
        .with_defaults_from(&with_a_capitalised_document())
        .expect("the default config serialises");
    let defaults_first = described()
        .with_defaults_from(&with_a_capitalised_document())
        .expect("the default config serialises")
        .refine(DOCUMENTS, slugs())
        .expect("applies");
    assert_eq!(refine_first, defaults_first);

    let documents = key(&refine_first, DOCUMENTS);
    assert!(documents.required, "a `Terms` entry is refused at boot");
    assert_eq!(documents.default, None);
    assert_eq!(documents.default_value, None);

    // A default every name of which is a slug stays one.
    let kept = defaulted().refine(DOCUMENTS, slugs()).expect("applies");
    assert!(!key(&kept, DOCUMENTS).required);
    assert!(key(&kept, DOCUMENTS).default_value.is_some());
}

#[test]
fn the_json_schema_and_the_contract_hold_entry_names_to_the_pattern() {
    let refined = defaulted()
        .refine_with("legal", &LegalPages)
        .and_then(|schema| schema.refine(DOCUMENTS, slugs()))
        .expect("the refinements apply");
    let validator = document_validator(&refined.to_json_schema().expect("it renders"));

    let page = json!({"title": "x"});
    let both = json!({"imprint": page, "privacy": page});
    assert!(validator.is_valid(&json!({"legal": {"documents": both}})));
    let mut capitalised = both;
    capitalised["Terms"] = page;
    assert!(!validator.is_valid(&json!({"legal": {"documents": capitalised}})));

    let contract = refined
        .into_contract(App::new("portfolio"))
        .build()
        .expect("a refined schema builds");
    let document: Json =
        serde_json::from_str(&contract.to_json().expect("it serialises")).expect("JSON");
    assert_eq!(
        document["schema"]["schema_version"],
        json!(LATEST_SCHEMA_VERSION)
    );
    assert_eq!(
        contract.json_schema["properties"]["legal"]["properties"]["documents"]["propertyNames"],
        json!({"pattern": SLUG})
    );
}

#[test]
fn a_merge_is_written_at_the_later_version() {
    let refined = described().refine(DOCUMENTS, slugs()).expect("applies");
    let unrefined = Terrace::new("PORTFOLIO_").schema::<Workers>();
    assert_eq!(
        unrefined.clone().merge(refined.clone()).schema_version,
        LATEST_SCHEMA_VERSION
    );
    assert_eq!(
        refined.merge(unrefined).schema_version,
        LATEST_SCHEMA_VERSION
    );
}

#[test]
fn markdown_and_the_toml_example_say_what_the_names_must_match() {
    let refined = defaulted()
        .refine(DOCUMENTS, imprint_and_privacy())
        .and_then(|schema| schema.refine(DOCUMENTS, slugs()))
        .expect("the refinements apply");

    let markdown = refined.to_markdown();
    let row = markdown
        .lines()
        .find(|line| line.starts_with("| `legal.documents`"))
        .expect("the key has a row");
    assert!(
        row.contains(
            "`BTreeMap<String, LegalDocument>`, must contain: `imprint`, `privacy`, entry names \
             match `^[a-z0-9][a-z0-9_-]{0,63}$`"
        ),
        "{row}"
    );

    let example = refined.to_toml_example();
    assert!(
        example.contains(
            "# Must contain: imprint, privacy\n# Entry names match: ^[a-z0-9][a-z0-9_-]{0,63}$\n"
        ),
        "{example}"
    );
}

// ---- the producer refusal the refinement could otherwise violate ----

#[derive(Deserialize, Serialize, Describe)]
struct Workers {
    /// Worker threads; zero is not a pool.
    #[config(range(min = 1))]
    #[serde(default)]
    workers: u32,
}

#[test]
fn a_contract_refuses_a_default_its_own_constraint_rejects() {
    let schema = Terrace::new("POOL_")
        .schema::<Workers>()
        .with_defaults_from(&Workers { workers: 0 })
        .expect("the default config serialises");
    let message = match schema.into_contract(App::new("pool")).build() {
        Ok(_) => panic!("a default below its own minimum was published"),
        Err(error) => error.to_string(),
    };
    assert!(
        message.contains("`workers` publishes the default 0"),
        "{message}"
    );
    assert!(message.contains("below the minimum 1"), "{message}");
}

#[test]
fn a_hand_edited_required_list_is_caught_at_the_same_boundary() {
    // What a producer could do before `refine` existed: mutate the constraint by dotted path and
    // leave the default contradicting it. `build` is where that stops.
    let mut schema = defaulted();
    let documents = schema
        .keys
        .iter_mut()
        .find(|key| key.path == DOCUMENTS)
        .expect("the key is described");
    documents
        .constraint
        .as_mut()
        .and_then(Json::as_object_mut)
        .expect("an object constraint")
        .insert("required".to_owned(), json!(["imprint"]));

    let message = match schema.into_contract(App::new("portfolio")).build() {
        Ok(_) => panic!("a default the constraint rejects was published"),
        Err(error) => error.to_string(),
    };
    assert!(
        message.contains("it is missing the required entry `imprint`"),
        "{message}"
    );
}
