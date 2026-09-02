//! What a Rust type token says about the value a key takes.
//!
//! [`Key::ty`] is token text, not a resolved type — a derive has only tokens, so a type alias
//! arrives as its alias and `SecretString` arrives as itself. Every other rendering prints it
//! back unchanged for exactly that reason. The JSON Schema rendering cannot: `type` is the
//! keyword that makes an editor reject `port = "8080"`, and there is nowhere else for it to come
//! from.
//!
//! So this is the one place in the crate that *interprets* a token, and it is written to fail
//! closed. A spelling it recognises produces the strictest constraint that is certainly true —
//! `u16` is an integer in `0..=65535`, and a schema saying so rejects nothing a `u16` would have
//! accepted. A spelling it does not recognise produces **nothing**, which in JSON Schema means
//! "any value": a key whose type is a domain newtype validates as it did before, rather than
//! being rejected on a guess about what the newtype wraps.
//!
//! # The one thing a token cannot say
//!
//! `Vec<RouteConfig>` is an array — that much is in the spelling — and what one element of it
//! looks like is not, because `RouteConfig` is a name and this module has no type graph to look it
//! up in. Teaching it more names would not help: the next configuration has a different struct.
//!
//! So the element is supplied from outside, by the type that *does* know: [`interpret_with`] takes
//! the schema a `#[config(element)]` field reported and puts it where the walk stops. Everything
//! about the containers around it still comes from the spelling, which is why a map of sets of an
//! opted-in enum needs no special case — the walk was already reading both containers correctly
//! and only the bottom was blank.
//!
//! [`Key::ty`]: super::Key::ty

use serde_json::{Map, Value as Json, json};

use super::TextForm;

/// The JSON Schema keywords `ty` justifies, or [`None`] for a spelling with no certain meaning.
///
/// Keywords rather than a shape enum of this crate's own: they are the vocabulary the output is
/// written in, and `items`, `uniqueItems` and `additionalProperties` all carry a nested schema,
/// so a shape enum would end up re-deriving JSON Schema one variant at a time.
pub(super) fn interpret(ty: &str) -> Option<Map<String, Json>> {
    walk(ty, None)
}

/// The same keywords, with `element` standing in at the position the walk cannot name itself.
///
/// [`walk`] already reads a container correctly — `HashMap<String, HashSet<Method>>` is an object
/// of arrays of *something* — and the only thing missing is the something. It recurses down
/// exactly one path (a sequence's item, a map's value), so there is exactly one such position per
/// type, and `element` goes in it however deep the containers are stacked.
///
/// [`None`] when `ty` is not a container, which is a guard rather than a formality. A key typed as
/// a bare domain struct has no element position, and grafting the schema at the key itself would
/// say the key *is* one element — an object where the file holds an array. A hand-written
/// [`Describe`](super::Describe) reporting whatever type text it likes is what this protects
/// against; the derive rejects the same shape at compile time, with a message that can name the
/// field.
pub(super) fn interpret_with(ty: &str, element: &Map<String, Json>) -> Option<Map<String, Json>> {
    if !is_container(ty) {
        return None;
    }
    walk(ty, Some(element))
}

/// Whether `ty` names something with an element position — a sequence, a set or a map.
///
/// The wrappers `serde` sees through are transparent here for the reason they are transparent in
/// [`walk`]: `Option<Vec<T>>` is supplied as an array, and the `Option` is a fact about whether
/// the key has to be there at all rather than about what it holds.
fn is_container(ty: &str) -> bool {
    let ty = strip_reference(ty.trim());
    if ty.starts_with('[') && ty.ends_with(']') {
        return true;
    }

    let (head, args) = split_generic(ty);
    let Some(name) = head.rsplit("::").next() else {
        return false;
    };
    match (name, args.as_slice()) {
        ("Option" | "Box" | "Arc" | "Rc" | "RefCell" | "Cell" | "Mutex" | "RwLock", [inner]) => {
            is_container(inner)
        }
        ("Cow", [_lifetime, inner]) => is_container(inner),
        ("Vec" | "VecDeque" | "HashSet" | "BTreeSet", [_]) | ("HashMap" | "BTreeMap", [_, _]) => {
            true
        }
        _ => false,
    }
}

/// One spelling, read: the containers it is made of, and what it bottoms out in.
///
/// `element` is what the bottom is when the spelling itself cannot say — [`None`] for an ordinary
/// read, and the caller's element schema when a key reported one. It is carried down through every
/// container and every transparent wrapper, so it lands wherever the recursion stops.
fn walk(ty: &str, element: Option<&Map<String, Json>>) -> Option<Map<String, Json>> {
    let ty = strip_reference(ty.trim());

    // `[T; N]` and `[T]`, which have no head to look up.
    if let Some(inner) = ty.strip_prefix('[').and_then(|ty| ty.strip_suffix(']')) {
        let (item, len) = match inner.split_once(';') {
            Some((item, len)) => (item, len.trim().parse::<u64>().ok()),
            None => (inner, None),
        };
        let mut schema = sequence(item, false, element);
        if let Some(len) = len {
            // A fixed-length array is the one sequence whose length is part of its type, and
            // `serde` refuses a file supplying any other number of elements.
            schema.insert("minItems".to_owned(), json!(len));
            schema.insert("maxItems".to_owned(), json!(len));
        }
        return Some(schema);
    }

    let (head, args) = split_generic(ty);
    let name = head.rsplit("::").next()?;
    let schema = match (name, args.as_slice()) {
        // Wrappers `serde` sees straight through. `Option` is stripped by the derive already;
        // it is here because a hand-written `Describe` reports whatever text it likes.
        ("Option" | "Box" | "Arc" | "Rc" | "RefCell" | "Cell" | "Mutex" | "RwLock", [inner]) => {
            return walk(inner, element);
        }
        ("Cow", [_lifetime, inner]) => return walk(inner, element),

        ("bool", []) => json_map(&json!({ "type": "boolean" })),
        ("f32" | "f64", []) => json_map(&json!({ "type": "number" })),

        // A `char` is a string of exactly one character once it has been through `serde`, and
        // saying so rejects the `mode = "ab"` that a bare string type would let through.
        ("char", []) => json_map(&json!({ "type": "string", "minLength": 1, "maxLength": 1 })),

        // Every one of these deserialises from a TOML string, and several of them — `PathBuf`,
        // `Url`, `IpAddr` — are the ones a configuration actually holds.
        (
            "String" | "str" | "PathBuf" | "Path" | "OsString" | "OsStr" | "CString" | "CStr"
            | "SecretString" | "Url" | "Uuid" | "IpAddr" | "Ipv4Addr" | "Ipv6Addr" | "SocketAddr"
            | "SocketAddrV4" | "SocketAddrV6",
            [],
        ) => json_map(&json!({ "type": "string" })),

        ("Vec" | "VecDeque", [item]) => sequence(item, false, element),
        ("HashSet" | "BTreeSet", [item]) => sequence(item, true, element),

        // The key type is ignored on purpose: a TOML table's keys are strings whatever the map
        // is keyed by, so it constrains nothing that could be written in the file. Which is also
        // why `element` goes to the value and never to the key.
        ("HashMap" | "BTreeMap", [_key, value]) => {
            let mut schema = json_map(&json!({ "type": "object" }));
            if let Some(value) = walk(value, element) {
                schema.insert("additionalProperties".to_owned(), Json::Object(value));
            }
            schema
        }

        // The bottom of the walk. A spelling with no certain meaning is where a reported element
        // schema belongs — and where, without one, this module says nothing rather than guessing.
        (name, []) => match integer(name) {
            Some(schema) => schema,
            None => return element.cloned(),
        },
        _ => return element.cloned(),
    };
    Some(schema)
}

/// The keywords an integer spelling justifies, bounds included where they are exact.
///
/// Bounds only up to 32 bits. `u64::MAX` is not representable as an IEEE double, so a `maximum`
/// carrying it would be a *different* number than the one the type accepts — and a validator
/// reading it back would reject values the loader takes. A zero lower bound on an unsigned type
/// is exact at every width, and is the bound that actually catches a mistake.
fn integer(name: &str) -> Option<Map<String, Json>> {
    let (min, max) = match name {
        "u8" => (Some(json!(0)), Some(json!(u8::MAX))),
        "u16" => (Some(json!(0)), Some(json!(u16::MAX))),
        "u32" => (Some(json!(0)), Some(json!(u32::MAX))),
        "u64" | "u128" | "usize" => (Some(json!(0)), None),
        "NonZeroU8" => (Some(json!(1)), Some(json!(u8::MAX))),
        "NonZeroU16" => (Some(json!(1)), Some(json!(u16::MAX))),
        "NonZeroU32" => (Some(json!(1)), Some(json!(u32::MAX))),
        "NonZeroU64" | "NonZeroU128" | "NonZeroUsize" => (Some(json!(1)), None),
        // The signed non-zero types share their bounds with the plain ones; what makes them
        // different is the hole in the middle, which `not` carries below.
        "i8" | "NonZeroI8" => (Some(json!(i8::MIN)), Some(json!(i8::MAX))),
        "i16" | "NonZeroI16" => (Some(json!(i16::MIN)), Some(json!(i16::MAX))),
        "i32" | "NonZeroI32" => (Some(json!(i32::MIN)), Some(json!(i32::MAX))),
        "i64" | "i128" | "isize" | "NonZeroI64" | "NonZeroI128" | "NonZeroIsize" => (None, None),
        _ => return None,
    };

    let mut schema = json_map(&json!({ "type": "integer" }));
    if let Some(min) = min {
        schema.insert("minimum".to_owned(), min);
    }
    if let Some(max) = max {
        schema.insert("maximum".to_owned(), max);
    }
    // The signed non-zero types are the only ones whose excluded value is interior to their
    // range, so it takes a keyword of its own rather than a tighter lower bound.
    if name.starts_with("NonZeroI") {
        schema.insert("not".to_owned(), json!({ "const": 0 }));
    }
    Some(schema)
}

/// What form the *unparsed text* of an environment variable takes, and the keywords that check
/// it.
///
/// [`interpret`] describes a value once it is in the document — an integer, a boolean, an array.
/// A variable holds none of those; it holds text, and `"0"` fails `{"type": "integer"}` under
/// every conforming validator. A consumer told only the document-space constraint has to know
/// that `u64` means "parse the text first, TOML-ish, then check", which is the Rust-type
/// vocabulary this module exists to stop publishing.
///
/// Every rule below was measured against the loader rather than reasoned from TOML's grammar,
/// because figment's `Env` provider is what decides this and its parse is neither TOML's nor
/// `str::parse`'s. Against a `u64` key it accepts `0`, `42`, `007`, `+5` and `7` with surrounding
/// whitespace, and refuses `1_000`, `0x1F`, `0b1` and `1e3` — so the pattern permits leading
/// zeros, a leading `+` and outer whitespace, and does not permit the separators and radix
/// prefixes TOML itself would. Against a `bool` it accepts `true` and `false` and nothing else:
/// not `TRUE`, not `1`, not `yes`.
///
/// Erring towards permissive is not a stylistic choice here. A pattern that rejects text the
/// loader accepts stops a deployment that was correct, which is the failure this whole module is
/// written to avoid — so a spelling whose accepted set is not known exactly gets [`None`].
///
/// # Why a string type gets nothing
///
/// `{"type": "string"}` is what every environment value already is, so emitting it would be a
/// constraint that constrains nothing. The interesting rule for a string-typed key is the
/// opposite one — figment parses `12345678` into a number and `Figment::extract` will not coerce
/// it back, so an all-digit password supplied this way fails to load (see
/// [`provider`](crate::provider)). Expressing that needs a `not` over the forms that parse, and
/// the boundary of "what parses" is exactly what is not known exactly enough to reject on. It is
/// documented instead of guessed.
pub(super) fn in_text(ty: &str) -> (TextForm, Option<Map<String, Json>>) {
    let ty = strip_reference(ty.trim());

    // `[T; N]` and `[T]` have no head to look up, and are sequences like the rest.
    if ty.starts_with('[') && ty.ends_with(']') {
        return (TextForm::Structured, Some(bracketed()));
    }

    let (head, args) = split_generic(ty);
    let Some(name) = head.rsplit("::").next() else {
        return (TextForm::Unknown, None);
    };

    match (name, args.as_slice()) {
        ("Option" | "Box" | "Arc" | "Rc" | "RefCell" | "Cell" | "Mutex" | "RwLock", [inner]) => {
            in_text(inner)
        }
        ("Cow", [_lifetime, inner]) => in_text(inner),

        ("bool", []) => (TextForm::Boolean, Some(one_of(&["true", "false"]))),

        // Accepts any text, and the loader agrees: none of these parses on the way in, so there
        // is nothing a validator could have caught. `Text` says exactly that — no check possible
        // and none needed.
        (
            "String" | "str" | "PathBuf" | "Path" | "OsString" | "OsStr" | "CString" | "CStr"
            | "SecretString",
            [],
        ) => (TextForm::Text, None),

        // A string in TOML and a *parse* in Rust, which is a different thing. Measured against the
        // loader: given `!!!`, an `IpAddr` key fails with "invalid IP address syntax", a
        // `SocketAddr` with "invalid socket address syntax", and a `char` with "expected a
        // character" — while `String` and `PathBuf` take it.
        //
        // So `Text` here would be the same defect `TextForm` was introduced to fix, one field
        // over: a value meaning "checked and fine" used for values that were never checked. This
        // crate has no pattern for a URL and would rather publish nothing than publish a wrong
        // one, so the honest answer is that a check exists and this document does not describe it.
        //
        // `Uuid` and `Ipv4Addr` are a few characters of regex and could move back to a described
        // form later; the rule for doing that is the rule everywhere else in this module — measure
        // what the loader accepts first, and emit a superset or nothing.
        (
            "Url" | "Uuid" | "IpAddr" | "Ipv4Addr" | "Ipv6Addr" | "SocketAddr" | "SocketAddrV4"
            | "SocketAddrV6" | "char",
            [],
        ) => (TextForm::Unknown, None),

        ("Vec" | "VecDeque" | "HashSet" | "BTreeSet", [_]) | ("HashMap" | "BTreeMap", [_, _]) => {
            (TextForm::Structured, Some(bracketed()))
        }

        (name, []) => match integer_text(name) {
            Some(schema) => (TextForm::Integer, Some(schema)),
            None => (TextForm::Unknown, None),
        },
        _ => (TextForm::Unknown, None),
    }
}

/// A fixed set of spellings, as the *text* of an environment variable — which is the same set
/// with the whitespace figment's `Env` provider strips before anything compares it.
///
/// A bare `enum` is what belongs in [`interpret`]'s output and it is wrong here, which is the
/// whole reason the two constraints are separate fields. Measured: `PROBE_LEVEL="info "`,
/// `" info"` and `"\tinfo\n"` all load, and so do `"true "` and `" false"` for a boolean — the
/// provider trims. The *document* layer trims nothing, so `level = "info "` in a TOML file really
/// is refused, and `constraint` stays a bare `enum` for exactly that reason.
///
/// A pattern rather than a widened enum because there is no widened enum to write: the set of
/// strings that trim to a variant is infinite.
pub(super) fn one_of(values: &[impl AsRef<str>]) -> Map<String, Json> {
    let alternatives: Vec<String> = values.iter().map(|value| escape(value.as_ref())).collect();
    json_map(&json!({
        "type": "string",
        "pattern": format!(r"^\s*({})\s*$", alternatives.join("|")),
    }))
}

/// `value` as a regular expression matching itself.
///
/// JSON Schema's `pattern` is ECMA-262, where a backslash before any of these is always the
/// literal character — so escaping the whole set is correct without knowing which of them a
/// `#[serde(rename = "…")]` might contain. Getting this wrong in the permissive direction would
/// accept a spelling the loader refuses; getting it wrong in the strict direction would reject a
/// deployment that works, which is the one this module is written to avoid.
fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if r"\^$.|?*+()[]{}".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// The one thing certainly true of a sequence or a map in an environment variable: it is a TOML
/// literal, so it is bracketed.
///
/// Measured, like the rest. Against a `Vec<String>` key figment takes `[]` and `["a","b"]` and
/// refuses `a,b`, `a` and the empty string — so `PORTFOLIO_GITHUB__REPOS=a,b`, which is the first
/// thing anyone would try, is caught here rather than at boot. The pattern says nothing about what
/// is *between* the brackets, because that is TOML's grammar and this is not a TOML parser.
fn bracketed() -> Map<String, Json> {
    json_map(&json!({
        "type": "string",
        // `[\s\S]` rather than `.`, which does not match a newline in most engines — and a
        // multi-line array is legal TOML, so a pattern that refused one would refuse a value the
        // loader takes.
        "pattern": r"^\s*[\[\{][\s\S]*[\]\}]\s*$",
    }))
}

/// The text form of an integer spelling: a sign, digits, and the whitespace figment trims.
///
/// No bounds. A pattern cannot express a numeric range, and half-expressing one — digits capped
/// at five characters for a `u16`, say — would reject `00080`, which the loader takes. The range
/// lives in [`interpret`]'s output and applies to the parsed value; this applies to the text, and
/// the two are complementary rather than alternatives.
fn integer_text(name: &str) -> Option<Map<String, Json>> {
    // Reuses the same table, so a spelling this crate stops recognising stops being described in
    // both places at once rather than in one of them.
    integer(name)?;

    // Unsigned by name: the loader refuses `-1` for one, and refusing it here catches the sign a
    // chart put in the wrong place before the pod does.
    let signed = !name.starts_with('u') && !name.starts_with("NonZeroU");
    let sign = if signed { "[-+]?" } else { r"\+?" };

    Some(json_map(&json!({
        "type": "string",
        "pattern": format!(r"^\s*{sign}[0-9]+\s*$"),
    })))
}

/// An array of `item`, with the element schema when `item` is a spelling this understands — or
/// when the key reported one, which is what `element` carries.
fn sequence(item: &str, unique: bool, element: Option<&Map<String, Json>>) -> Map<String, Json> {
    let mut schema = json_map(&json!({ "type": "array" }));
    if let Some(item) = walk(item, element) {
        schema.insert("items".to_owned(), Json::Object(item));
    }
    if unique {
        schema.insert("uniqueItems".to_owned(), json!(true));
    }
    schema
}

/// `&str` and `&'a str` are the same type as `str` to `serde`, and neither is spelled that way.
fn strip_reference(ty: &str) -> &str {
    let Some(rest) = ty.strip_prefix('&') else {
        return ty;
    };
    let rest = rest.trim_start();
    let Some(lifetime) = rest.strip_prefix('\'') else {
        return rest;
    };
    // `'a str` — everything up to the first space is the lifetime.
    lifetime
        .split_once(char::is_whitespace)
        .map_or(rest, |(_, ty)| ty.trim_start())
}

/// `Vec<String>` as `("Vec", ["String"])`, and a type with no arguments as itself.
///
/// Split at the *top* level only, so `HashMap<String, Vec<u8>>` is two arguments rather than
/// three. Brackets and parentheses count towards the depth as well as angles, because
/// `[(u8, u8); 2]` reaches here through the array branch.
fn split_generic(ty: &str) -> (&str, Vec<&str>) {
    let Some(open) = ty.find('<') else {
        return (ty, Vec::new());
    };
    let Some(inner) = ty.strip_suffix('>') else {
        return (ty, Vec::new());
    };
    let inner = &inner[open + 1..];

    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in inner.char_indices() {
        match character {
            '<' | '[' | '(' => depth += 1,
            '>' | ']' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                args.push(inner[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    args.push(inner[start..].trim());
    (ty[..open].trim(), args)
}

/// The object a `json!` literal already is, as the map this module passes around.
fn json_map(value: &Json) -> Map<String, Json> {
    match value {
        Json::Object(map) => map.clone(),
        // Every caller passes an object literal, so this is unreachable rather than a case with
        // a meaningful answer.
        _ => Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value as Json, json};

    use super::{interpret, interpret_with};

    /// The kind of schema a `#[config(element)]` field reports: something this module could never
    /// have produced from the token text.
    fn element() -> Map<String, Json> {
        match json!({ "type": "object", "properties": { "id": { "type": "string" } } }) {
            Json::Object(map) => map,
            _ => unreachable!("the literal is an object"),
        }
    }

    #[test]
    fn an_unrecognised_spelling_constrains_nothing() {
        assert_eq!(interpret("LogLevel"), None);
        assert_eq!(interpret("crate::db::Pool"), None);
    }

    /// A path spelling is the same type as its last segment, which is what a derive may or may
    /// not have been given.
    #[test]
    fn a_path_is_read_by_its_last_segment() {
        assert_eq!(interpret("std::path::PathBuf"), interpret("PathBuf"));
    }

    #[test]
    fn an_unsigned_integer_cannot_be_negative() {
        let schema = interpret("u16").expect("`u16` is understood");
        assert_eq!(schema["type"], json!("integer"));
        assert_eq!(schema["minimum"], json!(0));
        assert_eq!(schema["maximum"], json!(65_535));
    }

    /// A 64-bit bound is not representable as an IEEE double, so it is left off rather than
    /// written down wrong.
    #[test]
    fn a_sixty_four_bit_bound_is_left_off() {
        let schema = interpret("u64").expect("`u64` is understood");
        assert_eq!(schema["minimum"], json!(0));
        assert!(!schema.contains_key("maximum"), "{schema:?}");
    }

    #[test]
    fn a_sequence_carries_its_element_type() {
        let schema = interpret("Vec<String>").expect("`Vec<String>` is understood");
        assert_eq!(schema["type"], json!("array"));
        assert_eq!(schema["items"], json!({ "type": "string" }));
    }

    #[test]
    fn a_set_is_a_sequence_without_repeats() {
        let schema = interpret("BTreeSet<u8>").expect("`BTreeSet<u8>` is understood");
        assert_eq!(schema["uniqueItems"], json!(true));
    }

    #[test]
    fn a_map_is_an_object_over_its_value_type() {
        let schema = interpret("HashMap<String, Vec<u8>>").expect("the map is understood");
        assert_eq!(schema["type"], json!("object"));
        assert_eq!(schema["additionalProperties"]["type"], json!("array"));
    }

    #[test]
    fn a_fixed_length_array_carries_its_length() {
        let schema = interpret("[u8; 4]").expect("the array is understood");
        assert_eq!(schema["minItems"], json!(4));
        assert_eq!(schema["maxItems"], json!(4));
    }

    #[test]
    fn a_reported_element_fills_the_position_a_token_cannot_name() {
        let schema = interpret_with("Vec<RouteConfig>", &element()).expect("a sequence");
        assert_eq!(schema["type"], json!("array"));
        assert_eq!(schema["items"], Json::Object(element()));
    }

    /// Both containers were already read correctly; only the bottom was blank, so nothing about
    /// this case is special beyond how deep the recursion goes.
    #[test]
    fn a_map_of_sets_reaches_the_element_through_both() {
        let schema =
            interpret_with("HashMap<String, HashSet<Method>>", &element()).expect("a map of sets");
        assert_eq!(schema["type"], json!("object"));
        let set = &schema["additionalProperties"];
        assert_eq!(set["type"], json!("array"));
        assert_eq!(set["uniqueItems"], json!(true));
        assert_eq!(set["items"], Json::Object(element()));
    }

    /// The `Option` is a fact about whether the key has to be there, not about what it holds.
    #[test]
    fn a_wrapped_container_is_still_a_container() {
        assert_eq!(
            interpret_with("Option<Vec<Entry>>", &element()),
            interpret_with("Vec<Entry>", &element())
        );
        assert_eq!(
            interpret_with("[Entry; 2]", &element()).expect("a fixed array")["items"],
            Json::Object(element())
        );
    }

    /// A key that is not a container has no element position, and putting the schema at the key
    /// itself would claim the key *is* one element.
    #[test]
    fn a_type_that_is_not_a_container_refuses_an_element() {
        assert_eq!(interpret_with("RouteConfig", &element()), None);
        assert_eq!(interpret_with("String", &element()), None);
    }

    /// A container whose element the spelling already names keeps that reading. Nothing in the
    /// crate produces this pairing — `Vec<String>` cannot report a describing element — but the
    /// walk must not let a reported schema overwrite a certainty.
    #[test]
    fn a_named_element_is_not_replaced_by_a_reported_one() {
        assert_eq!(
            interpret_with("Vec<String>", &element()),
            interpret("Vec<String>")
        );
    }

    #[test]
    fn a_wrapper_is_the_type_it_wraps() {
        assert_eq!(interpret("Option<String>"), interpret("String"));
        assert_eq!(interpret("Arc<Vec<u8>>"), interpret("Vec<u8>"));
        assert_eq!(interpret("Cow<'a,str>"), interpret("String"));
        assert_eq!(interpret("&'a str"), interpret("String"));
    }
}
