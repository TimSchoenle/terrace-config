//! Unit tests over the expansion: the generated tokens for representative inputs, and the error
//! each rejected input gets.

use quote::quote;
use syn::{DeriveInput, Type};

use crate::attrs::doc_comment;
use crate::container::{Container, RenameRule};
use crate::expand;
use crate::types::{KNOWN_LEAVES, is_option, type_text, unwrap_option};

/// The generated `describe` body for `input`, whitespace-normalised so an assertion can be
/// written the way the code reads rather than the way `TokenStream::to_string` spaces it.
fn generated(input: &str) -> String {
    let parsed: DeriveInput = syn::parse_str(input).expect("test input is valid Rust");
    let tokens = expand(&parsed)
        .expect("test input describes cleanly")
        .to_string();
    tokens.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The error `input` is rejected with.
fn rejected(input: &str) -> String {
    let parsed: DeriveInput = syn::parse_str(input).expect("test input is valid Rust");
    expand(&parsed)
        .expect_err("test input is meant to be rejected")
        .to_string()
}

/// The `///` comments on the first field of `input`.
fn field_docs(input: &str) -> String {
    let parsed: DeriveInput = syn::parse_str(input).expect("test input is valid Rust");
    let syn::Data::Struct(data) = &parsed.data else {
        panic!("test input is a struct")
    };
    doc_comment(&data.fields.iter().next().expect("one field").attrs)
}

// ---- the shapes that have no keys to describe ----

#[test]
fn a_tuple_struct_is_rejected_because_its_fields_have_no_names() {
    let error = rejected("struct S(u8);");
    assert!(
        error.contains("cannot be derived for a tuple struct"),
        "{error}"
    );
    assert!(error.contains("struct of named fields"), "{error}");
}

#[test]
fn a_unit_struct_is_rejected() {
    assert!(rejected("struct S;").contains("cannot be derived for a unit struct"));
}

#[test]
fn a_union_is_rejected() {
    assert!(rejected("union U { a: u8 }").contains("cannot be derived for a union"));
}

/// A struct with no fields at all is legal and describes nothing: an empty configuration section is
/// a real thing, and refusing it would be refusing a shape serde accepts.
#[test]
fn a_struct_with_no_fields_describes_nothing() {
    let body = generated("struct S {}");
    assert!(body.contains("fn describe"), "{body}");
    assert!(!body.contains("sink . leaf"), "{body}");
}

// ---- attribute combinations that cannot mean anything ----

#[test]
fn flatten_without_nested_is_rejected_with_the_fix() {
    let error = rejected("struct S { #[serde(flatten)] inner: I }");
    assert!(
        error.contains("must also be `#[config(nested)]`"),
        "{error}"
    );
    assert!(error.contains("`#[config(skip)]`"), "{error}");
}

#[test]
fn a_secret_subtree_is_rejected_because_a_subtree_has_no_value() {
    let error = rejected("struct S { #[config(nested, secret)] inner: I }");
    assert!(error.contains("describes a subtree"), "{error}");
}

#[test]
fn a_noted_subtree_is_rejected_for_the_same_reason() {
    let error = rejected("struct S { #[config(nested, note = \"x\")] inner: I }");
    assert!(error.contains("describes a subtree"), "{error}");
}

/// The rename this attribute went through has a message of its own, because "unknown option"
/// would not say that the prose also has to be reworded.
#[test]
fn the_old_default_attribute_names_its_replacement() {
    let error = rejected("struct S { #[config(default = \"0 (permanent)\")] a: u8 }");
    assert!(
        error.contains("is now `#[config(note = \"…\")]`"),
        "{error}"
    );
    assert!(
        error.contains("should become just \"permanent\""),
        "{error}"
    );
}

#[test]
fn an_unknown_field_option_lists_the_ones_that_exist() {
    let error = rejected("struct S { #[config(sercet)] a: u8 }");
    assert!(error.contains("unknown `#[config(...)]` option"), "{error}");
    assert!(error.contains("`nested`"), "{error}");
    assert!(error.contains("`note = \"…\"`"), "{error}");
}

// ---- the interval a type cannot state ----

#[test]
fn a_range_emits_the_four_json_schema_keywords_it_was_given() {
    let body = generated("struct S { #[config(range(min = 0.0, exclusive_max = 1.0))] rate: f32 }");
    assert!(body.contains("Bounds"), "{body}");
    assert!(body.contains("Bound :: Fractional (0f64)"), "{body}");
    assert!(body.contains("Bound :: Fractional (1f64)"), "{body}");
    assert!(body.contains("min_exclusive : false"), "{body}");
    assert!(body.contains("max_exclusive : true"), "{body}");
}

/// An integer literal stays an integer all the way to the emitted schema, so a bound past an
/// `f64`'s exact range is never rounded on the way through.
#[test]
fn an_integer_bound_keeps_its_literal_kind() {
    let body = generated("struct S { #[config(range(min = 1, max = 63))] workers: u16 }");
    assert!(body.contains("Bound :: Integer (1i64)"), "{body}");
    assert!(body.contains("Bound :: Integer (63i64)"), "{body}");
}

/// A negation is a unary expression rather than part of the literal, so it has to be unwrapped, or
/// the error names the wrong problem.
#[test]
fn a_negative_bound_is_a_literal_like_any_other() {
    let body = generated("struct S { #[config(range(min = -5))] drift: i16 }");
    assert!(body.contains("Bound :: Integer (- 5i64)"), "{body}");
}

/// A field that reported no interval must emit exactly what it emitted before, which is what
/// keeps a contract using neither attribute serialising byte for byte as it did.
#[test]
fn a_field_without_a_range_reports_none() {
    let body = generated("struct S { a: u8 }");
    assert!(
        body.contains("bounds : :: core :: option :: Option :: None"),
        "{body}"
    );
}

#[test]
fn a_range_with_no_bounds_is_rejected() {
    let error = rejected("struct S { #[config(range())] a: u8 }");
    assert!(error.contains("says nothing about the value"), "{error}");
    assert!(error.contains("`exclusive_max`"), "{error}");
}

#[test]
fn an_unknown_bound_lists_the_four_that_exist() {
    let error = rejected("struct S { #[config(range(floor = 0))] a: u8 }");
    assert!(
        error.contains("unknown `#[config(range(...))]` bound"),
        "{error}"
    );
}

/// Two answers to one question, which is the silent wrong answer this derive refuses
/// everywhere else.
#[test]
fn one_end_of_a_range_cannot_be_set_twice() {
    let error = rejected("struct S { #[config(range(min = 0, exclusive_min = 1))] a: u8 }");
    assert!(error.contains("`min` and `exclusive_min`"), "{error}");
    assert!(error.contains("sets it twice"), "{error}");
}

#[test]
fn a_non_numeric_bound_is_rejected() {
    let error = rejected("struct S { #[config(range(min = \"0\"))] a: u8 }");
    assert!(error.contains("a numeric literal"), "{error}");
}

#[test]
fn a_range_that_accepts_nothing_is_rejected() {
    let error = rejected("struct S { #[config(range(min = 10, max = 1))] a: u8 }");
    assert!(error.contains("`min` is above `max`"), "{error}");
    assert!(error.contains("accepts no value at all"), "{error}");
}

#[test]
fn a_bounded_choice_is_rejected_because_a_key_is_one_or_the_other() {
    let error = rejected("struct S { #[config(values, range(min = 0))] a: Level }");
    assert!(error.contains("fixed set of spellings"), "{error}");
}

/// Both describe the position one element of the container holds, and an element is a shape or
/// a number rather than both.
#[test]
fn a_bounded_element_is_rejected_and_names_the_attribute_it_collides_with() {
    let error = rejected("struct S { #[config(element, range(min = 0))] a: Vec<Route> }");
    assert!(error.contains("`#[config(element)]`"), "{error}");
    assert!(error.contains("describe the same position"), "{error}");
}

#[test]
fn a_bounded_subtree_is_rejected_for_the_reason_a_secret_one_is() {
    let error = rejected("struct S { #[config(nested, range(min = 0))] inner: I }");
    assert!(error.contains("describes a subtree"), "{error}");
    assert!(error.contains("`range`"), "{error}");
}

// ---- a struct that refuses a key it did not declare ----

#[test]
fn deny_unknown_fields_is_reported_from_the_serde_attribute() {
    let body = generated("#[serde(deny_unknown_fields)] struct S { a: u8 }");
    assert!(body.contains("sink . deny_unknown_fields ()"), "{body}");
}

/// `serde` accepts a field nobody declared unless the struct says otherwise, so silence is the
/// answer for a struct that said nothing, and the feature never becomes a default.
#[test]
fn a_struct_that_did_not_say_so_reports_nothing() {
    let body = generated("struct S { a: u8 }");
    assert!(!body.contains("deny_unknown_fields"), "{body}");
}

#[test]
fn an_unknown_container_option_says_only_crate_exists() {
    let error = rejected("#[config(nested)] struct S { a: u8 }");
    assert!(error.contains("on a struct"), "{error}");
    assert!(error.contains("`crate = \"…\"`"), "{error}");
}

#[test]
fn an_unknown_rename_rule_is_named_in_the_error() {
    let error = rejected("#[serde(rename_all = \"SpongeCase\")] struct S { a: u8 }");
    assert!(
        error.contains("`SpongeCase` is not a serde rename rule"),
        "{error}"
    );
}

#[test]
fn a_non_string_attribute_value_is_rejected() {
    let error = rejected("struct S { #[config(note = 7)] a: u8 }");
    assert!(error.contains("expected a string literal"), "{error}");
}

#[test]
fn an_unparseable_crate_path_is_rejected() {
    let error = rejected("#[config(crate = \"not a path\")] struct S { a: u8 }");
    assert!(error.contains("not a crate path"), "{error}");
}

// ---- serde attributes this has to read, and the ones it must leave alone ----

/// Every rule serde has. A key path that disagrees with the one serde looks for documents a
/// key nobody can set, which is worse than documenting nothing.
#[test]
fn every_serde_rename_rule_matches_serde() {
    for (rule, expected) in [
        ("lowercase", "max_connections"),
        ("UPPERCASE", "MAX_CONNECTIONS"),
        ("PascalCase", "MaxConnections"),
        ("camelCase", "maxConnections"),
        ("snake_case", "max_connections"),
        ("SCREAMING_SNAKE_CASE", "MAX_CONNECTIONS"),
        ("kebab-case", "max-connections"),
        ("SCREAMING-KEBAB-CASE", "MAX-CONNECTIONS"),
    ] {
        let body = generated(&format!(
            "#[serde(rename_all = \"{rule}\")] struct S {{ max_connections: u32 }}"
        ));
        assert!(
            body.contains(&format!("name : \"{expected}\"")),
            "{rule}: {body}"
        );
    }
}

/// A rule applied to a single-segment name, where `PascalCase` and `camelCase` differ only in
/// the first character and an off-by-one in `pascal` would not show up above.
#[test]
fn a_single_segment_name_still_distinguishes_pascal_from_camel() {
    assert_eq!(RenameRule::Pascal.apply("port"), "Port");
    assert_eq!(RenameRule::Camel.apply("port"), "port");
    assert_eq!(RenameRule::Pascal.apply(""), "");
    assert_eq!(RenameRule::Camel.apply(""), "");
    // A trailing or doubled underscore produces empty segments, which `pascal` must drop
    // rather than index into.
    assert_eq!(RenameRule::Pascal.apply("a__b_"), "AB");
}

/// Only the deserialising half can change which key a configuration file has to spell.
#[test]
fn the_deserialize_half_of_a_split_rename_is_the_one_read() {
    let body =
        generated("struct S { #[serde(rename(serialize = \"out\", deserialize = \"in\"))] a: u8 }");
    assert!(body.contains("name : \"in\""), "{body}");

    let body = generated(
        "#[serde(rename_all(serialize = \"UPPERCASE\", deserialize = \"kebab-case\"))]
         struct S { max_connections: u8 }",
    );
    assert!(body.contains("name : \"max-connections\""), "{body}");
}

#[test]
fn both_spellings_of_serde_skip_omit_the_key() {
    assert!(!generated("struct S { #[serde(skip)] a: u8 }").contains("sink . leaf"));
    assert!(!generated("struct S { #[serde(skip_deserializing)] a: u8 }").contains("sink . leaf"));
}

#[test]
fn a_container_level_serde_default_makes_every_field_optional() {
    let body = generated("#[serde(default)] struct S { a: u8, b: u8 }");
    assert!(!body.contains("required : true"), "{body}");
    assert_eq!(body.matches("required : false").count(), 2, "{body}");
}

#[test]
fn a_container_level_serde_default_path_does_too() {
    let body = generated("#[serde(default = \"d\")] struct S { a: u8 }");
    assert!(body.contains("required : false"), "{body}");
}

/// Rejecting an option serde understands would make this derive a reason not to upgrade
/// serde. Every one of these carries a value shape the parser has to step over.
#[test]
fn serde_options_this_does_not_care_about_are_left_alone() {
    let body = generated(
        "#[serde(deny_unknown_fields, bound = \"T: Clone\", expecting = \"a struct\")]
         struct S {
             #[serde(with = \"mod_path\", alias = \"b\", skip_serializing_if = \"Option::is_none\")]
             a: u8,
             #[serde(borrow, getter(serialize = \"x\"))]
             c: u8,
         }",
    );
    assert!(body.contains("name : \"a\""), "{body}");
    assert!(body.contains("name : \"c\""), "{body}");
}

#[test]
fn a_raw_identifier_is_described_by_the_name_it_serialises_as() {
    let body = generated("struct S { r#type: u8 }");
    assert!(body.contains("name : \"type\""), "{body}");
}

#[test]
fn the_crate_can_be_renamed() {
    let body = generated("#[config(crate = \"::vendored::tc\")] struct S { a: u8 }");
    assert!(
        body.contains(":: vendored :: tc :: schema :: Describe"),
        "{body}"
    );
    assert!(!body.contains("terrace_config"), "{body}");
}

#[test]
fn generics_are_carried_onto_the_impl() {
    let body = generated("struct S<T: Copy> where T: Send { #[config(nested)] a: T }");
    assert!(body.contains("impl < T : Copy >"), "{body}");
    assert!(body.contains("where T : Send"), "{body}");
}

// ---- what a `///` comment survives as ----

#[test]
fn a_doc_comment_is_dedented_by_exactly_one_space() {
    assert_eq!(field_docs("struct S { /// One.\n a: u8 }"), "One.");
    // Further indentation is the author's, and a code block inside a doc comment depends on
    // it, so only the single leading space rustc inserts is removed.
    assert_eq!(
        field_docs("struct S { ///     indented\n a: u8 }"),
        "    indented"
    );
}

#[test]
fn blank_lines_inside_a_doc_comment_are_kept_but_the_outer_ones_are_not() {
    let docs = field_docs("struct S { ///\n /// One.\n ///\n /// Two.\n ///\n a: u8 }");
    assert_eq!(docs, "One.\n\nTwo.");
}

#[test]
fn a_field_with_no_doc_comment_reports_an_empty_string_rather_than_inventing_prose() {
    assert_eq!(field_docs("struct S { a: u8 }"), "");
}

/// `#[doc = "…"]` is what `///` desugars to, so both have to work: a macro-generated struct carries
/// the explicit form.
#[test]
fn the_explicit_doc_attribute_is_read_too() {
    assert_eq!(
        field_docs("struct S { #[doc = \"Explicit.\"] a: u8 }"),
        "Explicit."
    );
}

// ---- `Option<_>`, which decides whether a key is required ----

#[test]
fn option_is_recognised_however_it_is_spelled() {
    assert!(is_option(&syn::parse_quote!(Option<u8>)));
    assert!(is_option(&syn::parse_quote!(std::option::Option<u8>)));
    assert!(is_option(&syn::parse_quote!(
        ::core::option::Option<String>
    )));
}

#[test]
fn what_is_not_an_option_is_required() {
    assert!(!is_option(&syn::parse_quote!(u8)));
    assert!(!is_option(&syn::parse_quote!(Vec<u8>)));
    // An alias is one identifier and indistinguishable from any other; `#[serde(default)]`
    // is what such a field would carry anyway.
    assert!(!is_option(&syn::parse_quote!(MaybeString)));
    // Not the `Option` anyone means, and not one with a single type argument.
    assert!(!is_option(&syn::parse_quote!(Option)));
    assert!(!is_option(&syn::parse_quote!(Option<'a, u8>)));
    assert!(!is_option(&syn::parse_quote!(<T as Trait>::Option)));
}

#[test]
fn a_nested_option_is_described_through_to_its_inner_type() {
    let outer: Type = syn::parse_quote!(Option<Inner>);
    let inner = unwrap_option(&outer).expect("an Option");
    assert_eq!(quote!(#inner).to_string(), "Inner");

    let body = generated("struct S { #[config(nested)] a: Option<Inner> }");
    assert!(body.contains("< Inner as"), "{body}");
}

#[test]
fn a_container_parses_its_defaults_when_it_has_no_attributes() {
    let parsed: DeriveInput = syn::parse_str("struct S { a: u8 }").expect("valid");
    let container = Container::parse(&parsed).expect("no attributes to reject");
    assert!(!container.field_default);
    assert_eq!(container.rename_all.apply("a_b"), "a_b");
}

// ---- an enum is a set of values, not a set of keys ----

#[test]
fn an_enum_of_unit_variants_reports_the_values_it_accepts() {
    let body = generated("enum L { Trace, Debug }");
    assert!(body.contains("schema :: Values for L"), "{body}");
    assert!(body.contains(r#"& ["Trace" , "Debug"]"#), "{body}");
    // It is a value set, not a key set, so it must not claim to describe keys.
    assert!(!body.contains("Describe"), "{body}");
}

#[test]
fn an_enum_with_no_variants_accepts_nothing_rather_than_failing() {
    let body = generated("enum Never {}");
    assert!(body.contains("schema :: Values"), "{body}");
    assert!(body.contains("& []"), "{body}");
}

/// A variant carrying data is a shape rather than a choice, and has no single spelling a
/// configuration file could hold.
#[test]
fn a_variant_carrying_data_is_rejected() {
    for shape in ["enum E { A(u8) }", "enum E { A { b: u8 } }"] {
        let error = rejected(shape);
        assert!(error.contains("has to *be* one value"), "{shape}: {error}");
    }
}

#[test]
fn variant_renaming_is_not_field_renaming() {
    // `snake_case` has to *insert* the underscores a field name already has.
    let body = generated(r#"#[serde(rename_all = "snake_case")] enum E { PlainOld }"#);
    assert!(body.contains(r#""plain_old""#), "{body}");
    // And `PascalCase` is the identity for a variant, where for a field it is a conversion.
    let body = generated(r#"#[serde(rename_all = "PascalCase")] enum E { PlainOld }"#);
    assert!(body.contains(r#""PlainOld""#), "{body}");
}

#[test]
fn a_unicode_variant_name_survives_the_snake_case_rule() {
    assert_eq!(
        RenameRule::Snake.apply_to_variant("ÜberCache"),
        "über_cache"
    );
    assert_eq!(RenameRule::Snake.apply_to_variant(""), "");
    assert_eq!(RenameRule::Snake.apply_to_variant("A"), "a");
}

// ---- the type column ----

#[test]
fn a_type_is_reported_the_way_it_is_written() {
    for (declared, expected) in [
        ("String", "String"),
        ("u16", "u16"),
        ("Vec<String>", "Vec<String>"),
        ("Vec<Vec<u8>>", "Vec<Vec<u8>>"),
        ("std::path::PathBuf", "std::path::PathBuf"),
        ("BTreeMap<String, u8>", "BTreeMap<String, u8>"),
        ("[u8; 4]", "[u8; 4]"),
        ("(u8, String)", "(u8, String)"),
        // The `Option` is stripped, because `required` already carries that.
        ("Option<Vec<String>>", "Vec<String>"),
    ] {
        let body = generated(&format!("struct S {{ a: {declared} }}"));
        assert!(
            body.contains(&format!(
                r#"ty : :: core :: option :: Option :: Some ("{expected}")"#
            )),
            "{declared}: {body}"
        );
    }
}

/// A reference type keeps its lifetime, which `TokenStream` prints with a space in it.
#[test]
fn a_borrowed_type_is_tidied_too() {
    let borrowed: Type = syn::parse_quote!(&'a str);
    assert_eq!(type_text(&borrowed), "&'a str");
}

// ---- choices and aliases ----

#[test]
fn config_values_pulls_the_variants_off_the_field_type() {
    let body = generated("struct S { #[config(values)] a: LogLevel }");
    assert!(
        body.contains("< LogLevel as :: terrace_config :: schema :: Values >"),
        "{body}"
    );
    // Through an `Option`, the variants belong to the inner type.
    let body = generated("struct S { #[config(values)] a: Option<LogLevel> }");
    assert!(body.contains("< LogLevel as"), "{body}");
}

#[test]
fn a_subtree_cannot_be_given_choices() {
    let error = rejected("struct S { #[config(nested, values)] a: Inner }");
    assert!(error.contains("describes a subtree"), "{error}");
    assert!(error.contains("enumerate"), "{error}");
}

#[test]
fn every_serde_alias_is_collected_in_order() {
    let body = generated(r#"struct S { #[serde(alias = "user", alias = "login")] a: String }"#);
    assert!(body.contains(r#"aliases : & ["user" , "login"]"#), "{body}");
}

#[test]
fn a_field_with_no_alias_reports_an_empty_list() {
    assert!(generated("struct S { a: u8 }").contains("aliases : & []"));
}

#[test]
fn values_is_listed_among_the_field_options_in_the_error() {
    assert!(rejected("struct S { #[config(sercet)] a: u8 }").contains("`values`"));
}

// ---- what one element of a container-typed key holds ----

#[test]
fn a_container_reports_its_element_through_describe() {
    let body = generated("struct S { #[config(element)] a: Vec<Route> }");
    assert!(body.contains("sink . repeated"), "{body}");
    assert!(
        body.contains("Element :: Fields (< Route as :: terrace_config :: schema :: Describe >"),
        "{body}"
    );
    // Still one key: `repeated` is `leaf` with the element attached, not a second entry.
    assert!(body.contains(r#"name : "a""#), "{body}");
    assert_eq!(body.matches("sink .").count(), 1, "{body}");
}

#[test]
fn a_container_of_a_choice_reports_its_variants() {
    let body = generated("struct S { #[config(element_values)] a: BTreeSet<Method> }");
    assert!(
        body.contains("Element :: Choice (< Method as :: terrace_config :: schema :: Values >"),
        "{body}"
    );
}

/// The element is at the bottom of the containers, not one step in: every level above it is
/// read from the tokens, and only the bottom is blank.
#[test]
fn the_element_is_found_through_stacked_containers() {
    for declared in [
        "Vec<Route>",
        "Option<Vec<Route>>",
        "HashMap<String, Route>",
        "HashMap<String, HashSet<Route>>",
        "Arc<BTreeMap<u8, Box<Vec<Route>>>>",
        "[Route; 4]",
        "std::collections::BTreeMap<String, Route>",
        "Cow<'a, Vec<Route>>",
    ] {
        let body = generated(&format!("struct S {{ #[config(element)] a: {declared} }}"));
        assert!(body.contains("< Route as"), "{declared}: {body}");
    }
}

/// A map's *key* type is skipped for the reason the runtime walk skips it: a TOML table's keys
/// are strings whatever the map is keyed by.
#[test]
fn a_maps_key_type_is_never_the_element() {
    let body = generated("struct S { #[config(element)] a: BTreeMap<RouteName, Route> }");
    assert!(body.contains("< Route as"), "{body}");
    assert!(!body.contains("RouteName as"), "{body}");
}

/// Guessing would be silent and wrong: a schema saying `routes` is an object because its element is
/// a struct describes a file nobody can write.
#[test]
fn an_element_on_something_that_is_not_a_container_is_rejected() {
    let error = rejected("struct S { #[config(element)] a: Route }");
    assert!(error.contains("`#[config(element)]`"), "{error}");
    assert!(error.contains("not a container"), "{error}");
    assert!(error.contains("`Sink::repeated`"), "{error}");
}

/// The trap this error exists for: the type *is* a container, and a derive cannot see through
/// a name to know it.
#[test]
fn a_type_alias_for_a_container_is_rejected_with_the_reason() {
    let error = rejected("struct S { #[config(element_values)] a: Methods }");
    assert!(error.contains("`#[config(element_values)]`"), "{error}");
    assert!(error.contains("type alias"), "{error}");
}

#[test]
fn an_element_cannot_be_two_shapes_at_once() {
    for order in [
        "struct S { #[config(element, element_values)] a: Vec<T> }",
        "struct S { #[config(element_values)] #[config(element)] a: Vec<T> }",
    ] {
        let error = rejected(order);
        assert!(
            error.contains("describe the same element"),
            "{order}: {error}"
        );
    }
}

/// Repeating one attribute says nothing new, which is not the same as saying two things.
#[test]
fn repeating_the_same_element_attribute_is_harmless() {
    let body = generated("struct S { #[config(element)] #[config(element)] a: Vec<Route> }");
    assert!(body.contains("< Route as"), "{body}");
}

#[test]
fn an_element_does_not_combine_with_the_attributes_that_describe_the_field_itself() {
    for combination in ["nested, element", "values, element_values"] {
        let error = rejected(&format!(
            "struct S {{ #[config({combination})] a: Vec<T> }}"
        ));
        assert!(
            error.contains("describes what a container of that type holds"),
            "{combination}: {error}"
        );
    }
}

#[test]
fn the_element_options_are_listed_among_the_field_options_in_the_error() {
    let error = rejected("struct S { #[config(sercet)] a: u8 }");
    assert!(error.contains("`element`"), "{error}");
    assert!(error.contains("`element_values`"), "{error}");
}

// ---- a named type that describes nothing ----

/// The failure this diagnostic exists for: the field looks annotated enough, and publishes a
/// key with no shape at all.
#[test]
fn an_undescribed_named_type_is_rejected_naming_the_field_and_every_fix() {
    let error = rejected("struct S { log_level: LogLevel }");
    assert!(
        error.contains("`log_level: LogLevel` publishes no shape at all"),
        "{error}"
    );
    for attribute in [
        "`#[config(values)]`",
        r#"`#[config(values_from = "…")]`"#,
        r#"`#[config(values("…", "…"))]`"#,
        "`#[config(nested)]`",
        "`#[config(element)]`",
        "`#[config(element_values)]`",
        "`#[config(range(…))]`",
        "`#[config(skip)]`",
    ] {
        assert!(error.contains(attribute), "{attribute}: {error}");
    }
}

/// The same rule one level down, reported at the position that is actually blank.
#[test]
fn an_undescribed_element_is_rejected_and_names_the_element() {
    let error = rejected("struct S { routes: Vec<Route> }");
    assert!(error.contains("`routes: Vec<Route>`"), "{error}");
    assert!(error.contains("one element of it is `Route`"), "{error}");
}

/// Every spelling `terrace_config` reads a shape out of on its own. A leaf added there and
/// forgotten here is a field this derive would refuse for no reason.
#[test]
fn every_known_leaf_needs_no_attribute() {
    for leaf in KNOWN_LEAVES {
        let body = generated(&format!("struct S {{ a: {leaf} }}"));
        assert!(body.contains("sink . leaf"), "{leaf}: {body}");
    }
}

/// A leaf is matched by the last segment of its path and through the wrappers serde sees
/// straight through, exactly as the runtime walk matches it.
#[test]
fn a_leaf_is_recognised_however_it_is_reached() {
    for declared in [
        "std::path::PathBuf",
        "Option<String>",
        "&'a str",
        "Box<Option<u16>>",
        "Cow<'a, str>",
    ] {
        let body = generated(&format!("struct S<'a> {{ a: {declared} }}"));
        assert!(body.contains("sink . leaf"), "{declared}: {body}");
    }
}

#[test]
fn a_container_of_leaves_needs_no_attribute() {
    for declared in [
        "Vec<String>",
        "HashMap<String, u16>",
        "Option<Vec<PathBuf>>",
        "[u8; 4]",
        "BTreeMap<String, HashSet<f64>>",
    ] {
        let body = generated(&format!("struct S {{ a: {declared} }}"));
        assert!(body.contains("sink . leaf"), "{declared}: {body}");
    }
}

/// Each of the five, on the shape it applies to. Any one of them is an answer to the question
/// the diagnostic asks, so any one of them has to end it.
#[test]
fn each_resolving_attribute_silences_the_diagnostic() {
    for attribute in [
        "values",
        r#"values("a", "b")"#,
        r#"values_from = "LevelDef""#,
        "nested",
        "skip",
        "range(min = 0.0, max = 1.0)",
    ] {
        let input = format!("struct S {{ #[config({attribute})] a: Level }}");
        let parsed: DeriveInput = syn::parse_str(&input).expect("test input is valid Rust");
        assert!(expand(&parsed).is_ok(), "{attribute}");
    }
    for attribute in [
        "element",
        "element_values",
        r#"element_values("a", "b")"#,
        r#"element_values_from = "LevelDef""#,
    ] {
        let input = format!("struct S {{ #[config({attribute})] a: Vec<Level> }}");
        let parsed: DeriveInput = syn::parse_str(&input).expect("test input is valid Rust");
        assert!(expand(&parsed).is_ok(), "{attribute}");
    }
}

/// A tuple, a qualified path and an unrecognised generic publish nothing either, and none of the
/// five attributes is an answer for them, so an error naming them would be a dead end.
#[test]
fn a_shape_that_is_not_a_bare_name_is_left_alone() {
    for declared in ["(u8, String)", "<T as Trait>::Assoc", "Wrapper<Inner>"] {
        let body = generated(&format!("struct S {{ a: {declared} }}"));
        assert!(body.contains("sink . leaf"), "{declared}: {body}");
    }
}

/// The documented case for `range`: a domain newtype over a number, where the annotated
/// interval is the whole of what a schema can say. It publishes something, so the diagnostic
/// has nothing to complain about.
#[test]
fn a_bounded_newtype_publishes_its_interval_rather_than_nothing() {
    let body = generated("struct S { #[config(range(min = 0.0, max = 1.0))] ratio: Fraction }");
    assert!(body.contains("Bound :: Fractional (0f64)"), "{body}");
    assert!(
        body.contains(r#"ty : :: core :: option :: Option :: Some ("Fraction")"#),
        "{body}"
    );
}

/// `skip` is the escape hatch, and it must not become the answer to everything: a field that
/// takes it is left out of the schema entirely.
#[test]
fn skip_resolves_it_by_omitting_the_key() {
    assert!(!generated("struct S { #[config(skip)] a: Level }").contains("sink . leaf"));
}

// ---- values a trait cannot reach ----

/// The orphan rule puts `impl Values for tracing::Level` out of reach in every crate that
/// would want to write it, so a list is what a foreign enum has instead.
#[test]
fn a_literal_value_list_bypasses_the_values_trait() {
    let body = generated(r#"struct S { #[config(values("trace", "debug"))] a: tracing::Level }"#);
    assert!(
        body.contains(r#"values : :: core :: option :: Option :: Some (& ["trace" , "debug"])"#),
        "{body}"
    );
    // Nothing at all is asked of the type, which is the point.
    assert!(
        !body.contains("as :: terrace_config :: schema :: Values"),
        "{body}"
    );
    // And the type column still reports what was written.
    assert!(body.contains(r#"Some ("tracing::Level")"#), "{body}");
}

/// The bare form is unchanged, which is what keeps every existing consumer compiling.
#[test]
fn the_bare_values_attribute_still_means_the_trait() {
    let body = generated("struct S { #[config(values)] a: LogLevel }");
    assert!(
        body.contains("< LogLevel as :: terrace_config :: schema :: Values > :: VARIANTS"),
        "{body}"
    );
}

#[test]
fn a_literal_element_list_reaches_a_container_of_a_foreign_enum() {
    let body = generated(
        r#"struct S {
            #[config(element_values("trace", "debug"))]
            a: HashMap<String, tracing::Level>
        }"#,
    );
    assert!(body.contains("sink . repeated"), "{body}");
    assert!(
        body.contains(r#"Element :: Choice (& ["trace" , "debug"])"#),
        "{body}"
    );
}

/// A literal element list is still an element list: the field has to be a container, or the
/// schema would say the key itself is one element.
#[test]
fn a_literal_element_list_still_requires_a_container() {
    let error = rejected(r#"struct S { #[config(element_values("a"))] a: Level }"#);
    assert!(error.contains("not a container"), "{error}");
}

#[test]
fn an_empty_value_list_is_rejected_because_it_accepts_nothing() {
    for attribute in ["values", "element_values"] {
        let error = rejected(&format!(
            "struct S {{ #[config({attribute}())] a: Vec<L> }}"
        ));
        assert!(error.contains("lists no value"), "{attribute}: {error}");
        assert!(
            error.contains("drop the parentheses"),
            "{attribute}: {error}"
        );
    }
}

#[test]
fn a_repeated_spelling_in_a_value_list_is_rejected() {
    let error = rejected(r#"struct S { #[config(values("a", "b", "a"))] x: Level }"#);
    assert!(error.contains("`a` is listed twice"), "{error}");
}

#[test]
fn a_value_list_holds_string_literals() {
    let error = rejected("struct S { #[config(values(7))] a: Level }");
    assert!(error.contains("each one is a string literal"), "{error}");
}

/// Two answers to one question, which is the silent wrong answer this derive refuses
/// everywhere else.
#[test]
fn two_different_value_sources_are_rejected() {
    for written in [
        r#"values, values("a")"#,
        r#"values, values_from = "Other""#,
        r#"values_from = "One", values_from = "Two""#,
    ] {
        let error = rejected(&format!("struct S {{ #[config({written})] x: Level }}"));
        assert!(
            error.contains("name the values this key accepts two different ways"),
            "{written}: {error}"
        );
    }

    for written in [
        r#"element_values, element_values("a")"#,
        r#"element_values, element_values_from = "Other""#,
    ] {
        let error = rejected(&format!("struct S {{ #[config({written})] x: Vec<L> }}"));
        assert!(
            error.contains("name the spellings one element accepts two different ways"),
            "{written}: {error}"
        );
    }
}

/// Repeating one attribute says nothing new, which is not the same as saying two things.
#[test]
fn repeating_the_same_value_list_is_harmless() {
    let body = generated(r#"struct S { #[config(values("a"))] #[config(values("a"))] x: Level }"#);
    assert!(body.contains(r#"& ["a"]"#), "{body}");
}

/// A list says the key holds one of a fixed set of spellings, which is the claim `range`
/// collides with however the set was arrived at.
#[test]
fn a_literal_value_list_still_collides_with_a_range() {
    let error = rejected(r#"struct S { #[config(values("a"), range(min = 0))] x: Level }"#);
    assert!(error.contains("fixed set of spellings"), "{error}");
}

// ---- a set of values another type already has ----

/// The automated half of the same problem: a `#[serde(remote = "…")]` mirror is a local enum
/// whose variants serde matches exactly, so pointing at it costs no string literal and cannot
/// drift from what serde accepts.
#[test]
fn values_from_reports_another_types_values_implementation() {
    let body = generated(r#"struct S { #[config(values_from = "LevelDef")] a: tracing::Level }"#);
    assert!(
        body.contains("< LevelDef as :: terrace_config :: schema :: Values > :: VARIANTS"),
        "{body}"
    );
    // Nothing is asked of the field's own type, which is the point.
    assert!(
        !body.contains("< tracing :: Level as :: terrace_config :: schema :: Values >"),
        "{body}"
    );
    // And the type column still says what the field is written as.
    assert!(body.contains(r#"Some ("tracing::Level")"#), "{body}");
}

#[test]
fn element_values_from_does_the_same_one_level_down() {
    let body = generated(
        r#"struct S {
            #[config(element_values_from = "LevelDef")]
            a: HashMap<String, tracing::Level>
        }"#,
    );
    assert!(body.contains("sink . repeated"), "{body}");
    assert!(
        body.contains("Element :: Choice (< LevelDef as :: terrace_config :: schema :: Values >"),
        "{body}"
    );
}

/// A path, spelled as a string the way `crate = "…"` and serde's own `with` are.
#[test]
fn values_from_takes_a_type_path() {
    let body = generated(r#"struct S { #[config(values_from = "a::b::LevelDef")] x: L }"#);
    assert!(body.contains("< a :: b :: LevelDef as"), "{body}");

    let error = rejected(r#"struct S { #[config(values_from = "not a type")] x: L }"#);
    assert!(error.contains("is not a type"), "{error}");
    assert!(error.contains(r#"`values_from = "LevelDef"`"#), "{error}");
}

// ---- a wire form serde does not derive ----

/// The trap `docs/SCHEMA.md` describes in prose. A derived variant list is serde's *derived* wire
/// form, and each of these says the wire form is something else.
#[test]
fn a_derived_value_list_is_refused_under_a_custom_deserialiser() {
    for written in ["with", "deserialize_with"] {
        let error = rejected(&format!(
            r#"struct S {{ #[serde({written} = "m")] #[config(values)] a: Level }}"#
        ));
        assert!(error.contains("`#[config(values)]`"), "{written}: {error}");
        assert!(
            error.contains(&format!(r#"`#[serde({written} = "…")]`"#)),
            "{written}: {error}"
        );
        assert!(
            error.contains(r#"`#[config(values_from = "…")]`"#),
            "{written}: {error}"
        );
        assert!(error.contains("`#[config(skip)]`"), "{written}: {error}");

        let error = rejected(&format!(
            r#"struct S {{ #[serde({written} = "m")] #[config(element_values)] a: Vec<L> }}"#
        ));
        assert!(
            error.contains(r#"`#[config(element_values_from = "…")]`"#),
            "{written}: {error}"
        );
    }
}

/// Both of the other two are the author saying what the conversion accepts, so neither is the
/// derive guessing and neither is refused.
#[test]
fn a_named_or_listed_set_survives_a_custom_deserialiser() {
    for attribute in [r#"values_from = "LevelDef""#, r#"values("trace", "info")"#] {
        let body = generated(&format!(
            r#"struct S {{ #[serde(with = "m")] #[config({attribute})] a: Level }}"#
        ));
        assert!(body.contains("sink . leaf"), "{attribute}: {body}");
    }
}

/// The container half of the same trap, and the one the documented example turns on:
/// `#[serde(try_from = "String")]` over a case-insensitive `FromStr` accepts spellings no
/// variant is named after.
#[test]
fn an_enum_whose_deserialize_is_a_conversion_cannot_report_its_variants() {
    for written in ["try_from", "from"] {
        let error = rejected(&format!(
            r#"#[serde({written} = "String")] enum Level {{ Trace, Info }}"#
        ));
        assert!(
            error.contains("reports its variants as the values a key accepts"),
            "{written}: {error}"
        );
        assert!(
            error.contains(&format!(r#"`#[serde({written} = "…")]`"#)),
            "{written}: {error}"
        );
        assert!(
            error.contains("naming what the conversion really accepts"),
            "{written}: {error}"
        );
    }
}

/// `remote` is the exception, and the reason `values_from` exists: serde matches a mirror's
/// variants exactly, so they are precisely what a file may hold.
#[test]
fn a_remote_mirror_enum_still_reports_its_variants() {
    let body = generated(
        r#"#[serde(remote = "other::Compression", rename_all = "lowercase")]
           enum CompressionDef { Gzip, Zstd }"#,
    );
    assert!(
        body.contains("schema :: Values for CompressionDef"),
        "{body}"
    );
    assert!(body.contains(r#"& ["gzip" , "zstd"]"#), "{body}");
}

/// A container with no element attribute is the case that has to keep generating exactly what
/// it generated before.
#[test]
fn a_container_that_says_nothing_is_still_a_plain_leaf() {
    let body = generated("struct S { a: Vec<String> }");
    assert!(body.contains("sink . leaf"), "{body}");
    assert!(!body.contains("repeated"), "{body}");
}
