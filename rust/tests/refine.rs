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
    App, Condition, Describe, Key, LATEST_SCHEMA_VERSION, Refine, Refinement, SCHEMA_VERSION,
    Schema,
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

/// The version `propertyNames` arrived in, and so what a document carrying it is published at.
const PROPERTY_NAMES_VERSION: u32 = 3;

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
    assert_eq!(refined.schema_version, PROPERTY_NAMES_VERSION);
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
        json!(PROPERTY_NAMES_VERSION)
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
        PROPERTY_NAMES_VERSION
    );
    assert_eq!(
        refined.merge(unrefined).schema_version,
        PROPERTY_NAMES_VERSION
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

// ---- patterns, and positions inside a key ----

/// terrace-legal's shape: every document is a map of locale to text, twice over.
#[derive(Deserialize, Serialize, Describe)]
struct Localised {
    /// The locale a page falls back to.
    #[serde(default = "english")]
    default_locale: String,
    /// Documents, by slug.
    #[config(element)]
    #[serde(default)]
    documents: BTreeMap<String, Page>,
    /// Labels, in order.
    #[serde(default)]
    tags: Vec<String>,
}

impl Default for Localised {
    fn default() -> Self {
        Self {
            default_locale: english(),
            documents: BTreeMap::new(),
            tags: Vec::new(),
        }
    }
}

fn english() -> String {
    "en".to_owned()
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Page {
    /// Heading, by locale.
    #[serde(default)]
    title: BTreeMap<String, String>,
    /// Text, by locale.
    #[serde(default)]
    body: BTreeMap<String, String>,
}

/// `LocaleTag::from_str`: a two- or three-letter language, an optional script, an optional region.
const LOCALE: &str = "^[A-Za-z]{2,3}(?:[-_][A-Za-z]{4})?(?:[-_](?:[A-Za-z]{2}|[0-9]{3}))?$";

fn localised() -> Schema {
    Terrace::new("SITE_").schema::<Localised>()
}

/// A default holding one document whose English body is `text`.
fn one_document(text: &str) -> Localised {
    let mut config = Localised::default();
    config.documents.insert(
        "terms".to_owned(),
        Page {
            title: BTreeMap::from([("en".to_owned(), "Terms".to_owned())]),
            body: BTreeMap::from([("en".to_owned(), text.to_owned())]),
        },
    );
    config
}

fn at<'a>(schema: &'a Json, pointer: &str) -> &'a Json {
    schema
        .pointer(pointer)
        .unwrap_or_else(|| panic!("{pointer} is in {schema}"))
}

#[test]
fn a_pattern_on_a_string_key_is_version_two_vocabulary() {
    let refined = localised()
        .refine("default_locale", Refinement::pattern(LOCALE))
        .expect("a string key accepts a pattern");
    assert_eq!(
        constraint(&refined, "default_locale")["pattern"],
        json!(LOCALE)
    );
    assert_eq!(refined.schema_version, SCHEMA_VERSION);
}

#[test]
fn a_path_continues_into_the_element_and_its_fields() {
    let refined = localised()
        .refine("documents.*.body", Refinement::entry_names(LOCALE))
        .and_then(|schema| schema.refine("documents.*.title", Refinement::entry_names(LOCALE)))
        .and_then(|schema| schema.refine("documents.*.body.*", Refinement::non_blank()))
        .and_then(|schema| schema.refine("tags.*", Refinement::non_blank()))
        .expect("every position is described");

    let documents = constraint(&refined, "documents");
    let body = at(&documents, "/additionalProperties/properties/body");
    assert_eq!(body["propertyNames"], json!({"pattern": LOCALE}));
    assert_eq!(
        body["additionalProperties"]["pattern"],
        json!(Refinement::NON_BLANK)
    );
    assert_eq!(
        at(
            &documents,
            "/additionalProperties/properties/title/propertyNames"
        ),
        &json!({"pattern": LOCALE})
    );
    assert_eq!(
        constraint(&refined, "tags")["items"]["pattern"],
        json!(Refinement::NON_BLANK)
    );
    assert_eq!(refined.schema_version, PROPERTY_NAMES_VERSION);
}

#[test]
fn a_position_the_type_did_not_describe_is_refused_with_the_reason() {
    for (path, refinement, reason) in [
        (
            "documents.*.bodyy",
            Refinement::non_blank(),
            "`bodyy` is not a field of `documents.*`, so `documents.*.bodyy` names nothing. Its \
             fields are `body`, `title`.",
        ),
        (
            "documents.terms.body",
            Refinement::non_blank(),
            "`documents` is a map, and `terms` would be one entry of it",
        ),
        (
            "default_locale.*",
            Refinement::non_blank(),
            "`default_locale` is neither a map nor a sequence",
        ),
        (
            "documents..body",
            Refinement::non_blank(),
            "has an empty segment",
        ),
        (
            "documents",
            Refinement::non_blank(),
            "`documents` is not a string",
        ),
        (
            "documents.*",
            Refinement::entry_names(LOCALE),
            "`documents.*` is not a map",
        ),
        (
            "tags",
            Refinement::entry_names(LOCALE),
            "`tags` is not a map",
        ),
    ] {
        let message = refusal(localised().refine(path, refinement));
        assert!(message.contains(reason), "{path}: {message}");
    }

    // A map whose element the walk did not read has nothing below it to address.
    let message = refusal(described().refine("legal.links.*.x", Refinement::non_blank()));
    assert!(
        message.contains("`legal.links.*` is not a struct, so it has no field `x`"),
        "{message}"
    );
    let message = refusal(described().refine("legal.docs.*.body", Refinement::non_blank()));
    assert!(
        message.contains("an alias of `legal.documents`"),
        "{message}"
    );
}

#[test]
fn an_entry_name_inside_an_element_is_held_to_every_layer_too() {
    let refined = localised()
        .refine("documents.*.body", Refinement::required_entries(["en"]))
        .expect("`en` is spellable below any document");
    assert_eq!(
        at(
            &constraint(&refined, "documents"),
            "/additionalProperties/properties/body/required"
        ),
        &json!(["en"])
    );

    let message =
        refusal(localised().refine("documents.*.body", Refinement::required_entries(["EN"])));
    assert!(
        message.contains("`EN` cannot be a required entry of `documents.*.body`"),
        "{message}"
    );
    assert!(message.contains("in lower case"), "{message}");
}

#[test]
fn a_second_pattern_is_refused_and_the_same_one_changes_nothing() {
    let once = localised()
        .refine("default_locale", Refinement::pattern(LOCALE))
        .expect("applies");
    assert_eq!(
        once.clone()
            .refine("default_locale", Refinement::pattern(LOCALE))
            .expect("applies again"),
        once
    );
    let message = refusal(once.refine("default_locale", Refinement::non_blank()));
    assert!(message.contains("is already matched against"), "{message}");

    let message = refusal(localised().refine("default_locale", Refinement::pattern(r"\S")));
    assert!(
        message.contains("cannot be the pattern of `default_locale`"),
        "{message}"
    );
    assert!(message.contains(r"`\S` names a different set"), "{message}");
}

#[test]
fn a_default_a_nested_refinement_rejects_is_not_a_default_in_either_order() {
    let non_blank = || Refinement::non_blank();
    let refine_first = localised()
        .refine("documents.*.body.*", non_blank())
        .expect("applies")
        .with_defaults_from(&one_document(" \u{3000} "))
        .expect("the default serialises");
    let defaults_first = localised()
        .with_defaults_from(&one_document(" \u{3000} "))
        .expect("the default serialises")
        .refine("documents.*.body.*", non_blank())
        .expect("applies");
    assert_eq!(refine_first, defaults_first);
    assert!(key(&refine_first, "documents").required);
    assert_eq!(key(&refine_first, "documents").default_value, None);

    // U+FEFF is not white space, so `trim()` leaves it and so does the pattern.
    let kept = localised()
        .with_defaults_from(&one_document("\u{feff}"))
        .expect("the default serialises")
        .refine("documents.*.body.*", non_blank())
        .expect("applies");
    assert!(!key(&kept, "documents").required);

    let capitalised = Localised {
        default_locale: "english!".to_owned(),
        ..Localised::default()
    };
    let locale = localised()
        .with_defaults_from(&capitalised)
        .expect("the default serialises")
        .refine("default_locale", Refinement::pattern(LOCALE))
        .expect("applies");
    assert!(key(&locale, "default_locale").required);
}

#[test]
fn the_json_schema_holds_every_document_to_the_nested_refinements() {
    let refined = localised()
        .refine("documents.*.body", Refinement::entry_names(LOCALE))
        .and_then(|schema| schema.refine("documents.*.body.*", Refinement::non_blank()))
        .expect("applies");
    let validator = document_validator(&refined.to_json_schema().expect("it renders"));

    assert!(validator.is_valid(&json!({"documents": {"terms": {"body": {"de_DE": "Text"}}}})));
    assert!(!validator.is_valid(&json!({"documents": {"terms": {"body": {"deutsch": "Text"}}}})));
    assert!(!validator.is_valid(&json!({"documents": {"terms": {"body": {"de": " \t"}}}})));
}

#[test]
fn the_renderings_say_where_inside_the_key_each_refinement_is() {
    let refined = localised()
        .refine("default_locale", Refinement::pattern(LOCALE))
        .and_then(|schema| schema.refine("documents.*.body", Refinement::entry_names(LOCALE)))
        .and_then(|schema| schema.refine("documents.*.body.*", Refinement::non_blank()))
        .expect("applies");

    let markdown = refined.to_markdown();
    let locale = markdown
        .lines()
        .find(|line| line.starts_with("| `default_locale`"))
        .expect("the key has a row");
    // A table cell escapes `|`, as it does in every other cell.
    let escaped = LOCALE.replace('|', r"\|");
    assert!(
        locale.contains(&format!("`String`, matches `{escaped}`")),
        "{locale}"
    );
    let documents = markdown
        .lines()
        .find(|line| line.starts_with("| `documents`"))
        .expect("the key has a row");
    assert!(
        documents.contains(&format!(
            "`BTreeMap<String, Page>`, at `*.body`: entry names match `{escaped}`, at `*.body.*`: \
             matches `"
        )),
        "{documents}"
    );

    let example = refined.to_toml_example();
    assert!(
        example.contains(&format!("# Matches: {LOCALE}\n")),
        "{example}"
    );
    assert!(
        example.contains(&format!("# Entry names match at *.body: {LOCALE}\n")),
        "{example}"
    );
    assert!(
        example.contains(&format!(
            "# Matches at *.body.*: {}\n",
            Refinement::NON_BLANK
        )),
        "{example}"
    );
}

// ---- conditions between fields ----

/// terrace-legal's `documents.<slug>`: hosted or external, and a consent rule on top.
#[derive(Deserialize, Serialize, Default, Describe)]
struct Legal2 {
    /// Documents, by slug.
    #[config(element)]
    #[serde(default)]
    documents: BTreeMap<String, Document>,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Document {
    /// Text, by locale. Empty for an external document.
    #[serde(default)]
    body: BTreeMap<String, String>,
    /// Where an external document lives.
    #[serde(default)]
    url: Option<String>,
    #[config(nested)]
    #[serde(default)]
    consent: Consent,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Consent {
    /// What a visitor is asked for.
    #[config(values)]
    #[serde(default)]
    requirement: Requirement,
    /// The version a visitor consents to.
    #[serde(default)]
    version: Option<String>,
    /// Days a changed document may still be shown without renewed consent.
    #[config(range(max = 365))]
    #[serde(default)]
    grace_days: u16,
    /// When the current version took effect.
    #[serde(default)]
    effective: Option<String>,
}

#[derive(Deserialize, Serialize, Default, Describe)]
#[serde(rename_all = "lowercase")]
enum Requirement {
    #[default]
    None,
    Accept,
}

fn legal2() -> Schema {
    Terrace::new("SITE_").schema::<Legal2>()
}

/// Exactly one of hosted and external.
fn hosted_or_external() -> Refinement {
    Refinement::holds(Condition::exactly_one([
        Condition::present("url"),
        Condition::non_empty("body"),
    ]))
}

/// A requirement needs a version and a hosted document, and a grace period needs an effective date —
/// but only under a requirement, since the runtime returns before any other check without one.
fn consent_rule() -> Refinement {
    Refinement::holds(Condition::when(
        Condition::not_equals("consent.requirement", "none"),
        Condition::all([
            Condition::absent("url"),
            Condition::matches("consent.version", Refinement::NON_BLANK),
            Condition::when(
                Condition::above("consent.grace_days", 0),
                Condition::present("consent.effective"),
            ),
        ]),
    ))
}

fn documents_with(document: &Json) -> Json {
    json!({"documents": {"terms": document}})
}

#[test]
fn a_condition_is_an_all_of_member_carrying_its_sentence_and_raises_the_version() {
    let refined = legal2()
        .refine("documents.*", hosted_or_external())
        .expect("both fields are declared");
    let element = constraint(&refined, "documents")["additionalProperties"].clone();
    assert_eq!(
        element["allOf"],
        json!([{
            "description": "exactly one of: [`url` is set; `body` is not empty]",
            "oneOf": [
                {"required": ["url"]},
                {"required": ["body"], "properties": {"body": {"minProperties": 1}}},
            ],
        }])
    );
    assert_eq!(refined.schema_version, LATEST_SCHEMA_VERSION);

    // Twice is once.
    let twice = refined
        .clone()
        .refine("documents.*", hosted_or_external())
        .expect("applies again");
    assert_eq!(twice, refined);
}

#[test]
fn the_json_schema_holds_every_document_to_its_conditions_exactly() {
    let refined = legal2()
        .refine("documents.*", hosted_or_external())
        .and_then(|schema| schema.refine("documents.*", consent_rule()))
        .expect("every field is declared");
    let validator = document_validator(&refined.to_json_schema().expect("it renders"));
    let valid = |document: Json| validator.is_valid(&documents_with(&document));

    assert!(valid(json!({"body": {"en": "x"}})), "hosted");
    assert!(valid(json!({"url": "https://example.com"})), "external");
    assert!(
        valid(json!({"url": "u", "body": {}})),
        "external, with an empty body"
    );
    assert!(!valid(json!({"url": "u", "body": {"en": "x"}})), "both");
    assert!(!valid(json!({})), "neither");
    assert!(!valid(json!({"body": {}})), "an empty body alone");

    let consent = |consent: Json| json!({"body": {"en": "x"}, "consent": consent});
    assert!(valid(consent(json!({"requirement": "none"}))));
    assert!(
        valid(consent(json!({"requirement": "none", "grace_days": 5}))),
        "no rule without a requirement"
    );
    assert!(
        !valid(consent(json!({"requirement": "accept"}))),
        "a requirement needs a version"
    );
    assert!(
        !valid(consent(json!({"requirement": "accept", "version": " "}))),
        "a blank version"
    );
    assert!(valid(consent(
        json!({"requirement": "accept", "version": "1"})
    )));
    assert!(
        !valid(consent(
            json!({"requirement": "accept", "version": "1", "grace_days": 5})
        )),
        "a grace period needs an effective date"
    );
    assert!(valid(consent(
        json!({"requirement": "accept", "version": "1", "grace_days": 5, "effective": "2026-09-22"})
    )));
    assert!(
        !valid(json!({"url": "u", "consent": {"requirement": "accept", "version": "1"}})),
        "a requirement needs a hosted document"
    );
}

#[test]
fn a_condition_names_only_declared_fields_on_a_struct() {
    for (path, refinement, reason) in [
        (
            "documents",
            hosted_or_external(),
            "`documents` is not a struct, so it has no fields for a condition to relate",
        ),
        (
            "documents.*",
            Refinement::holds(Condition::present("urll")),
            "the condition cannot be stated on `documents.*`: `urll` is not a field of \
             `documents.*`",
        ),
        (
            "documents.*",
            Refinement::holds(Condition::equals("consent.requirement", "always")),
            "`consent.requirement` can never be \"always\"",
        ),
        (
            "documents.*",
            Refinement::holds(Condition::above("url", 0)),
            "`url` is not a number",
        ),
    ] {
        let message = refusal(legal2().refine(path, refinement));
        assert!(message.contains(reason), "{path}: {message}");
    }
}

#[test]
fn a_default_a_condition_rejects_is_not_a_default_in_either_order() {
    let mut both = Legal2::default();
    both.documents.insert(
        "terms".to_owned(),
        Document {
            body: BTreeMap::from([("en".to_owned(), "x".to_owned())]),
            url: Some("https://example.com".to_owned()),
            consent: Consent::default(),
        },
    );
    let refine_first = legal2()
        .refine("documents.*", hosted_or_external())
        .expect("applies")
        .with_defaults_from(&both)
        .expect("the default serialises");
    let defaults_first = legal2()
        .with_defaults_from(&both)
        .expect("the default serialises")
        .refine("documents.*", hosted_or_external())
        .expect("applies");
    assert_eq!(refine_first, defaults_first);
    assert!(key(&refine_first, "documents").required);
    assert_eq!(key(&refine_first, "documents").default_value, None);
}

#[test]
fn a_contract_refuses_a_default_failing_a_condition_by_its_sentence() {
    // A condition written into the constraint by hand, with a default that fails it: `build` is the
    // boundary, and it names the rule rather than its encoding.
    let mut both = Legal2::default();
    both.documents
        .insert("terms".to_owned(), Document::default());
    let mut schema = legal2()
        .with_defaults_from(&both)
        .expect("the default serialises");
    let refined = legal2()
        .refine("documents.*", hosted_or_external())
        .expect("applies");
    schema
        .keys
        .iter_mut()
        .find(|key| key.path == "documents")
        .expect("described")
        .constraint = key(&refined, "documents").constraint.clone();
    let message = match schema.into_contract(App::new("site")).build() {
        Ok(_) => panic!("a default failing its own condition was published"),
        Err(error) => error.to_string(),
    };
    assert!(
        message.contains(
            "`terms` does not satisfy: exactly one of: [`url` is set; `body` is not empty]"
        ),
        "{message}"
    );
}

#[test]
fn the_renderings_say_each_condition_in_its_own_words() {
    let refined = legal2()
        .refine("documents.*", hosted_or_external())
        .expect("applies");
    let row = refined
        .to_markdown()
        .lines()
        .find(|line| line.starts_with("| `documents`"))
        .expect("the key has a row")
        .to_owned();
    assert!(
        row.contains(
            "`BTreeMap<String, Document>`, at `*`: exactly one of: [`url` is set; `body` is not \
             empty]"
        ),
        "{row}"
    );
    let example = refined.to_toml_example();
    assert!(
        example.contains("# Holds at *: exactly one of: [`url` is set; `body` is not empty]\n"),
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
