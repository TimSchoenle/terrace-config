//! The `Describe` derive — the compile-time half of `terrace-config`'s schema export.
//!
//! This crate exists only because a proc-macro crate cannot export anything else. Everything it
//! generates names types in `terrace_config::schema`, and it is depended on with `=` so the two
//! halves can never disagree about the shape of a `Leaf`. Depend on `terrace-config` with the
//! `schema` feature; never on this crate directly.
//!
//! The one thing only a macro can do is read the `///` comments. Every other column of a
//! configuration table — the key path, the environment spelling, whether a value is required —
//! is recoverable at runtime; the sentence explaining what the key is *for* exists nowhere but
//! the source, and vanishes before any runtime sees the type.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, ExprUnary, Field, Fields, GenericArgument, Lit,
    Meta, MetaList, Path, PathArguments, Token, Type, UnOp, parse_macro_input,
};

/// Describe a configuration type: its keys, their documentation, and what each one accepts.
///
/// Two shapes, two outputs:
///
/// - On a **struct of named fields** — a type that *has* configuration keys — this derives
///   `terrace_config::schema::Describe`, reporting one key per field.
/// - On an **enum of unit variants** — a type that *is* the set of values one key accepts — it
///   derives `terrace_config::schema::Values` instead, reporting the variant spellings. Pull them
///   into a key with `#[config(values)]` on the field that holds it.
///
/// A tuple struct, a unit struct, a union, or an enum whose variants carry data is none of those
/// and is rejected.
///
/// # Serde attributes
///
/// The key path a field produces has to be the one `serde` will actually look for, so the
/// following are read from the field's existing `#[serde(...)]` attributes rather than
/// duplicated: `rename`, `rename_all` (on the container, for fields *and* for variants — the two
/// rules differ and both are implemented), `alias`, `skip`, `skip_deserializing`, `default`, and
/// `flatten`. Annotating a field twice is how the documentation drifts from the loader.
///
/// `#[serde(deny_unknown_fields)]` on the container is read for the same reason and reported the
/// same way — see [Closed structs](#closed-structs).
///
/// # `#[config(...)]`
///
/// | Attribute | Effect |
/// |---|---|
/// | `#[config(nested)]` | Recurse into the field's type instead of treating it as a leaf |
/// | `#[config(secret)]` | Render the default as `<redacted>`, and mark the key in the output |
/// | `#[config(note = "…")]` | Annotate the observed default with prose |
/// | `#[config(values)]` | Report the field type's variants as the values the key accepts |
/// | `#[config(values("…", "…"))]` | Report a literal list as the values the key accepts |
/// | `#[config(range(…))]` | Bound the number the key accepts: `min`, `max`, `exclusive_min`, `exclusive_max` |
/// | `#[config(element)]` | Report the shape of one element of a container-typed key |
/// | `#[config(element_values)]` | Report the values one element of a container-typed key accepts |
/// | `#[config(element_values("…", "…"))]` | The same literal list, one level down |
/// | `#[config(skip)]` | Omit the key from the schema without affecting deserialisation |
/// | `#[config(crate = "…")]` | Name the `terrace_config` crate, if it was renamed |
///
/// `nested` is opt-in because no macro can tell a `PathBuf` from a nested config struct by
/// looking at the type: both are one identifier and a module path. Guessing would mean either
/// bare identifiers silently becoming leaves, or a bound on types that cannot satisfy it.
///
/// # A named type has to say something
///
/// Opt-in is not the same as optional. A field whose type is a bare name this crate does not
/// recognise — not one of the leaf spellings, not a container — publishes *nothing*: no type, no
/// values, no keys, and a row that looks exactly like a field which was described. That silence
/// is the gap this derive exists to close, so it is a compile error rather than an omission:
///
/// ```ignore
/// /// How much the service says.
/// log_level: LogLevel,   // error: publishes no shape at all
/// ```
///
/// Six attributes resolve it — `values`, `nested`, `element`, `element_values`, `range` for a
/// numeric newtype whose interval is the whole of what a schema can say about it, and `skip` for a
/// field that genuinely has no publishable shape. The error names the field, its type and all of
/// them.
///
/// It fires on a bare name only. A container's *element* is checked the same way one level down,
/// so `Vec<LogLevel>` is refused and `Vec<String>` is not.
///
/// # Values a trait cannot reach
///
/// `#[config(values)]` reads the field type's `Values` implementation, and the orphan rule puts a
/// foreign enum out of reach: an application cannot write `impl Values for tracing::Level`, and
/// this crate will not depend on `tracing` to write it here. A literal list bypasses the trait:
///
/// ```ignore
/// /// How much the service says.
/// #[config(values("trace", "debug", "info", "warn", "error"))]
/// #[serde(default)]
/// log_level: tracing::Level,
///
/// /// Levels each module is pinned to.
/// #[config(element_values("trace", "debug", "info", "warn", "error"))]
/// #[serde(default)]
/// module_levels: HashMap<String, tracing::Level>,
/// ```
///
/// **The list is an assertion this crate cannot check**, the same standing as
/// `#[config(note = "…")]`: nothing here reads the type's `Deserialize`, so a list that disagrees
/// with it publishes a schema rejecting a file the loader takes. It is the author's to keep true,
/// and it satisfies the diagnostic above. Bare `#[config(values)]` is unchanged and still means
/// "use the `Values` implementation". An empty list and a repeated spelling are rejected.
///
/// # Container-typed keys
///
/// `routes: Vec<RouteConfig>` is one key — an array index is not a key segment, and no
/// environment variable names one — but it is a key whose *element* has a shape, and the type
/// token `Vec<RouteConfig>` carries only half of it. `element` supplies the other half:
///
/// ```ignore
/// /// Routes declared in the file.
/// #[config(element)]
/// #[serde(default)]
/// routes: Vec<RouteConfig>,
///
/// /// Methods each path forwards.
/// #[config(element_values)]
/// #[serde(default)]
/// paths: HashMap<String, HashSet<Method>>,
/// ```
///
/// The element type is read off the container: through `Option`, `Box`, `Arc`, `Rc` and `Cow`,
/// into the item of a `Vec`, `VecDeque`, `HashSet`, `BTreeSet`, `[T]` or `[T; N]` and into the
/// *value* of a `HashMap` or `BTreeMap`, however deep they are stacked — `HashMap<String,
/// HashSet<Method>>` reaches `Method`. A field whose type is not one of those is an error rather
/// than a guess, and a type alias for a container is one of them: a derive has only tokens, so
/// `type Routes = Vec<RouteConfig>` is a bare identifier here and has to be spelled out.
///
/// `element` requires the element type to derive `Describe`; `element_values` requires it to
/// derive `Describe` as an enum, which is what produces `Values`. Neither combines with `nested`
/// or `values`, which describe the field's own type rather than its elements. The key itself is
/// reported exactly as it is without them — the schema gains a nested `items` or
/// `additionalProperties`, and not one extra key.
///
/// # Numeric bounds
///
/// `sample_rate: f32` publishes `{"type": "number"}` and nothing more. That the value is a
/// *fraction* is not in the type — every consumer that knows it has been writing `minimum: 0` and
/// `maximum: 1` itself. `range` says it once, where the field is:
///
/// ```ignore
/// /// Share of requests that are traced.
/// #[config(range(min = 0.0, max = 1.0))]
/// #[serde(default)]
/// sample_rate: f32,
///
/// /// Worker count, which must leave one core for the reactor.
/// #[config(range(min = 1, max = 63))]
/// #[serde(default)]
/// workers: u16,
///
/// /// Retry backoff multipliers, none of which may be a shrink.
/// #[config(range(exclusive_min = 1.0))]
/// #[serde(default)]
/// backoff: Vec<f64>,
/// ```
///
/// It takes `min`, `max`, `exclusive_min` and `exclusive_max` — at least one, and at most one per
/// end — and emits `minimum`, `maximum`, `exclusiveMinimum` and `exclusiveMaximum`. An integer
/// literal stays an integer in the output and a float stays a float, so a bound is never rounded
/// on its way into the schema.
///
/// **The bound lands where the type's own reading stops**, which for a container is the element:
/// `Vec<f64>` bounds the numbers in the vector, because a `minimum` on the vector itself would
/// mean nothing. That is the position `element` fills for a container of structs, reached by the
/// same walk — through any depth of `Option`, `Vec`, `HashMap` and the rest — which is why the two
/// do not combine.
///
/// **A bound is never allowed to widen one the type already justifies.** `max = 100_000` on a
/// `u16` is dropped rather than published, because `maximum` is one keyword and replacing the
/// exact `65535` with it would produce a schema accepting a file that cannot load.
///
/// # Closed structs
///
/// A struct reported through `element` publishes its `properties` and its `required`, which leaves
/// a consumer unable to tell it from an open map: a misspelt field passes validation. A container
/// whose element type carries `#[serde(deny_unknown_fields)]` now says so, with
/// `additionalProperties: false` beside those:
///
/// ```ignore
/// #[derive(Deserialize, Describe)]
/// #[serde(deny_unknown_fields)]
/// struct RouteConfig {
///     /// Where the route sends traffic.
///     upstream: String,
/// }
/// ```
///
/// Read from the serde attribute rather than a second annotation of this crate's own, so the
/// schema and the deserialiser cannot come to disagree about which fields exist. Nothing is
/// emitted for a struct without it — `serde` accepts an undeclared field by default, and a schema
/// refusing one would refuse a file that loads.
// `serde` is declared as a helper attribute as well as `config`. It is read, never consumed, and
// serde's own derives declare it too — which is allowed, and is what lets a struct carry
// `#[serde(rename_all = "…")]` under `Describe` alone. Without this, deriving `Describe` on a
// type that is not simultaneously `Deserialize` is a "cannot find attribute" error pointing at
// the user's serde attribute rather than at anything they did wrong.
#[proc_macro_derive(Describe, attributes(config, serde))]
pub fn derive_describe(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Two shapes, two outputs.
///
/// A struct of named fields *has* configuration keys, so it gets a `Describe` implementation. An
/// enum of unit variants *is* the set of values one key accepts, so it gets a `Values`
/// implementation instead — which is what lets a table print `trace | debug | info` rather than
/// naming a type whose inside the operator cannot see.
fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let container = Container::parse(input)?;
    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => expand_struct(input, &container, &named.named),
            Fields::Unnamed(_) => Err(unsupported(input, "a tuple struct")),
            Fields::Unit => Err(unsupported(input, "a unit struct")),
        },
        Data::Enum(data) => expand_enum(input, &container, data),
        Data::Union(_) => Err(unsupported(input, "a union")),
    }
}

/// The error for a shape that is neither a set of keys nor a set of choices.
fn unsupported(input: &DeriveInput, what: &str) -> syn::Error {
    syn::Error::new_spanned(
        &input.ident,
        format!(
            "`Describe` cannot be derived for {what}. It applies to a struct of named fields, \
             which has configuration keys, or to an enum of unit variants, which is the set of \
             values one key accepts — and {what} is neither."
        ),
    )
}

/// A struct's keys.
fn expand_struct(
    input: &DeriveInput,
    container: &Container,
    fields: &Punctuated<Field, Token![,]>,
) -> syn::Result<TokenStream2> {
    let krate = &container.krate;
    let mut body = TokenStream2::new();
    // First, so the level it closes is the one this type opened rather than one a field pushed.
    if container.deny_unknown_fields {
        body.extend(quote! { sink.deny_unknown_fields(); });
    }
    for field in fields {
        body.extend(field_tokens(field, container)?);
    }

    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #krate::schema::Describe for #ident #ty_generics #where_clause {
            fn describe(sink: &mut #krate::schema::Sink) {
                #body
            }
        }
    })
}

/// An enum's accepted values, spelled the way `serde` will accept them.
fn expand_enum(
    input: &DeriveInput,
    container: &Container,
    data: &syn::DataEnum,
) -> syn::Result<TokenStream2> {
    let mut variants = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                variant,
                "`Describe` on an enum reports the values a key accepts, so every variant has to \
                 *be* one value. A variant carrying data is a shape rather than a choice, and has \
                 no single spelling a configuration file could hold.",
            ));
        }

        let opts = VariantOpts::parse(&variant.attrs)?;
        if opts.skip {
            continue;
        }
        variants.push(opts.rename.unwrap_or_else(|| {
            container
                .rename_all
                .apply_to_variant(&variant.ident.to_string())
        }));
    }

    let krate = &container.krate;
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #krate::schema::Values for #ident #ty_generics #where_clause {
            const VARIANTS: &'static [&'static str] = &[#(#variants),*];
        }
    })
}

/// Options read from one enum variant's attributes.
#[derive(Default)]
struct VariantOpts {
    /// `#[serde(skip)]` or `#[serde(skip_deserializing)]`.
    skip: bool,
    /// `#[serde(rename = "…")]`, or its `deserialize` half.
    rename: Option<String>,
}

impl VariantOpts {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut opts = Self::default();
        for meta in attr_metas(attrs, "serde")? {
            match &meta {
                Meta::Path(path)
                    if path.is_ident("skip") || path.is_ident("skip_deserializing") =>
                {
                    opts.skip = true;
                }
                Meta::NameValue(nv) if nv.path.is_ident("rename") => {
                    opts.rename = Some(string_value(&nv.value)?);
                }
                Meta::List(list) if list.path.is_ident("rename") => {
                    for inner in nested_metas(list)? {
                        if let Meta::NameValue(nv) = &inner
                            && nv.path.is_ident("deserialize")
                        {
                            opts.rename = Some(string_value(&nv.value)?);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(opts)
    }
}

/// One field's contribution to the generated `describe` body.
fn field_tokens(field: &Field, container: &Container) -> syn::Result<TokenStream2> {
    let opts = FieldOpts::parse(field)?;
    if opts.skip {
        return Ok(TokenStream2::new());
    }

    let krate = &container.krate;
    let ty = &field.ty;

    if opts.nested {
        // `Option<Inner>` recurses into `Inner`: the keys underneath exist either way, and
        // `Option` itself has no fields to describe.
        let target = unwrap_option(ty).unwrap_or(ty);
        return Ok(if opts.flatten {
            // A flattened field contributes its keys at the current level, so no segment is
            // pushed. This is the one case where a field name never appears in a key path.
            quote! { <#target as #krate::schema::Describe>::describe(sink); }
        } else {
            let name = opts.name(field, container)?;
            quote! { sink.nested(#name, <#target as #krate::schema::Describe>::describe); }
        });
    }

    if opts.flatten {
        return Err(syn::Error::new_spanned(
            field,
            "a `#[serde(flatten)]` field must also be `#[config(nested)]`: flattening merges \
             another struct's keys into this one, so its fields are what appear in the schema. \
             Add `#[config(nested)]`, or `#[config(skip)]` to leave it undocumented.",
        ));
    }

    // After `skip`, after `nested` and after the `flatten` error, so a field that resolved its
    // type by any of those never reaches this — and the three attributes left are checked here.
    // `range` is one of them: a bound on a spelling this crate cannot read is carried alone, so
    // an annotated domain newtype publishes an interval rather than nothing.
    if opts.values.is_none() && opts.element.is_none() && opts.range.is_none() {
        describes_something(field, ty)?;
    }

    let name = opts.name(field, container)?;
    let docs = doc_comment(&field.attrs);
    let required = !(opts.has_serde_default || container.field_default || is_option(ty));
    let secret = opts.secret;
    let note = if let Some(text) = &opts.note {
        quote! { ::core::option::Option::Some(#text) }
    } else {
        quote! { ::core::option::Option::None }
    };

    // The `Option` is stripped: `required` already says whether the key may be left out, and
    // `Option<String>` in a type column says it a second time and less clearly. What an operator
    // actually has to supply is a `String`.
    let bare = unwrap_option(ty).unwrap_or(ty);
    let ty_text = type_text(bare);
    let values = optional(
        opts.values
            .as_ref()
            .map(|source| source.tokens(bare, krate)),
    );
    let aliases = &opts.aliases;
    let bounds = opts
        .range
        .map_or_else(|| optional(None), |range| range.tokens(krate));

    let leaf = quote! {
        #krate::schema::Leaf {
            name: #name,
            docs: #docs,
            ty: ::core::option::Option::Some(#ty_text),
            values: #values,
            bounds: #bounds,
            aliases: &[#(#aliases),*],
            note: #note,
            required: #required,
            secret: #secret,
        }
    };

    // A container-typed key is still one key. What changes is that the element the type token
    // cannot name is reported alongside it, and lands nested inside the key's constraint.
    let Some(element) = &opts.element else {
        return Ok(quote! { sink.leaf(#leaf); });
    };
    // Asked for even by the literal form, which has no use for the item type: the check is that
    // the field *is* a container, and reporting an element schema at a key that holds one value
    // would say the key is an array of itself.
    let item = element_type(bare).ok_or_else(|| not_a_container(field, element))?;
    let reported = match element {
        ElementKind::Fields => {
            quote! { #krate::schema::Element::Fields(<#item as #krate::schema::Describe>::describe) }
        }
        ElementKind::Choice(source) => {
            let variants = source.tokens(item, krate);
            quote! { #krate::schema::Element::Choice(#variants) }
        }
    };
    Ok(quote! { sink.repeated(#leaf, #reported); })
}

/// Refuse a field whose type publishes nothing and which was not told what it holds.
///
/// `values` and `nested` are opt-in and have to stay opt-in: a derive has only tokens, so it
/// cannot tell whether a named type implements `Values` or `Describe`, and guessing is unsound in
/// the direction that matters — a type whose `Deserialize` is `#[serde(try_from = "String")]` over
/// a case-insensitive `FromStr` accepts spellings that are not variants, and publishing the
/// variant list unasked would emit a schema rejecting a file the loader takes.
///
/// What a derive can do is refuse to be *silent*. `terrace_config` reads a leaf spelling it
/// recognises and publishes nothing for one it does not, so a field left at that publishes a key
/// with no shape — indistinguishable in the output from a key that was described. This is where
/// forgetting the attribute stops being an invisible gap and becomes a compile error.
///
/// Only a bare name is judged, at the position [`described_position`] stops on. A tuple, a
/// function pointer, a qualified path and a generic type this derive did not recognise as a
/// container publish nothing either, and none of them is a shape the resolving attributes have an
/// answer for — an error naming them would be a dead end rather than a fix.
fn describes_something(field: &Field, ty: &Type) -> syn::Result<()> {
    let position = described_position(ty);
    if !is_undescribed(position) {
        return Ok(());
    }

    let name = field
        .ident
        .as_ref()
        .map_or_else(|| "this field".to_owned(), ToString::to_string);
    let declared = type_text(ty);
    let position = type_text(position);
    let what = if position == declared {
        format!("`{position}` is a name")
    } else {
        format!("one element of it is `{position}`, which is a name")
    };

    Err(syn::Error::new_spanned(
        field,
        format!(
            "`{name}: {declared}` publishes no shape at all — {what}, and a derive has only \
             tokens, so nothing here says whether it is a set of values, a subtree of keys, a \
             number, or something with no publishable shape. A key documented as \"anything\" is \
             the gap this schema exists to close, so an attribute is required rather than \
             assumed: `#[config(values)]` if the type implements `Values`; \
             `#[config(values(\"…\", \"…\"))]` if it cannot — a foreign enum, which the orphan \
             rule puts out of reach — which asserts the accepted spellings the way \
             `#[config(note = \"…\")]` asserts prose; `#[config(nested)]` if it implements \
             `Describe` and its keys belong under this one; `#[config(element)]` or \
             `#[config(element_values)]` if the field is a container and it is the *element* \
             that is one of those; `#[config(range(…))]` if it is a number in an interval; and \
             `#[config(skip)]` if the field genuinely has nothing to publish."
        ),
    ))
}

/// Whether a type is a bare name `terrace_config` will read nothing from.
fn is_undescribed(ty: &Type) -> bool {
    let Type::Path(path) = ty else { return false };
    if path.qself.is_some() {
        return false;
    }
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    matches!(segment.arguments, PathArguments::None)
        && !KNOWN_LEAVES.contains(&segment.ident.to_string().as_str())
}

/// Every type spelling `terrace_config` reads a shape out of on its own.
///
/// The list `src/schema/rust_type.rs` matches on, restated here because a proc-macro crate cannot
/// depend on the crate it writes code for. Only *membership* has to agree — what each spelling
/// publishes is that module's business, and this one asks only whether it publishes anything —
/// and the two halves are held together by a test in `tests/schema.rs` that runs a field of every
/// one of these through both.
///
/// A path is matched by its last segment, exactly as the runtime walk matches it, so a fully
/// qualified `std::path::PathBuf` and a bare `PathBuf` are the same spelling and neither is
/// resolved.
const KNOWN_LEAVES: &[&str] = &[
    // Read as a string, because each deserialises from one.
    "String",
    "str",
    "PathBuf",
    "Path",
    "OsString",
    "OsStr",
    "CString",
    "CStr",
    "SecretString",
    "Url",
    "Uuid",
    "IpAddr",
    "Ipv4Addr",
    "Ipv6Addr",
    "SocketAddr",
    "SocketAddrV4",
    "SocketAddrV6",
    // A string of exactly one character, once serde has been through it.
    "char",
    "bool",
    "f32",
    "f64",
    // The integers, and the `NonZero` family that shares their bounds.
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "NonZeroU8",
    "NonZeroU16",
    "NonZeroU32",
    "NonZeroU64",
    "NonZeroU128",
    "NonZeroUsize",
    "NonZeroI8",
    "NonZeroI16",
    "NonZeroI32",
    "NonZeroI64",
    "NonZeroI128",
    "NonZeroIsize",
];

/// The spellings behind `values("trace", "debug")`, checked for the two ways such a list says
/// nothing.
///
/// Whether the list *matches the type* is not checkable here and is not checked: nothing in a
/// derive reads a foreign type's `Deserialize`. What is checkable is that the assertion is well
/// formed — an empty list claims the key accepts no value at all, and a repeated spelling is a
/// list that was edited in two places and would reach the schema's `enum` twice.
fn value_literals(list: &MetaList, attribute: &str) -> syn::Result<Vec<String>> {
    let mut values: Vec<String> = Vec::new();
    for expr in list.parse_args_with(Punctuated::<Expr, Token![,]>::parse_terminated)? {
        let Expr::Lit(ExprLit {
            lit: Lit::Str(literal),
            ..
        }) = &expr
        else {
            return Err(syn::Error::new_spanned(
                &expr,
                format!(
                    "`#[config({attribute}(…))]` lists the spellings a configuration file may \
                     hold, so each one is a string literal — \
                     `{attribute}(\"trace\", \"debug\")`."
                ),
            ));
        };

        let value = literal.value();
        if values.contains(&value) {
            return Err(syn::Error::new_spanned(
                literal,
                format!(
                    "`{value}` is listed twice, and a set of accepted values holds it once. A \
                     repeated spelling is a list edited in two places, and it would reach the \
                     schema's `enum` twice."
                ),
            ));
        }
        values.push(value);
    }

    if values.is_empty() {
        return Err(syn::Error::new_spanned(
            list,
            format!(
                "`#[config({attribute}())]` lists no value, so it says the key accepts nothing — \
                 a schema rejecting every configuration, including the ones that load. List the \
                 spellings, or drop the parentheses to report the type's own `Values` \
                 implementation."
            ),
        ));
    }

    Ok(values)
}

/// The error for `#[config(element)]` on a field this derive cannot find a container in.
///
/// Named rather than guessed at, because the guess would be silent and wrong: a schema saying
/// `routes` is an object because its element is a struct describes a file nobody can write. The
/// alias case is called out because it is the one that looks like a bug in the derive — the type
/// *is* a container, and a derive cannot see through a name to know it.
fn not_a_container(field: &Field, element: &ElementKind) -> syn::Error {
    let attribute = element.attribute();
    syn::Error::new_spanned(
        field,
        format!(
            "`#[config({attribute})]` says what one element of a container holds, and this \
             field's type is not a container this derive can read. Those are `Vec`, `VecDeque`, \
             `HashSet`, `BTreeSet`, `HashMap`, `BTreeMap`, `[T]` and `[T; N]`, through any \
             number of `Option`, `Box`, `Arc`, `Rc`, `Cow` and the other transparent wrappers. \
             A type alias for one of them is a bare identifier here, because a derive has only \
             tokens: spell the container out, or implement `Describe` by hand and call \
             `Sink::repeated`."
        ),
    )
}

/// The type one element of a container-typed field holds.
///
/// The walk `rust_type` performs over the type *token text* at runtime, performed here over the
/// tokens themselves: past the wrappers serde sees through, then into a sequence's item or a map's
/// value, until something that is not a container is reached. `HashMap<String,
/// HashSet<AllowedMethod>>` therefore yields `AllowedMethod` — which is exactly the position the
/// emitted constraint leaves open, because the two walks stop in the same place by construction.
///
/// [`None`] when there is no container to descend into. That is the answer for a type alias as
/// well, and [`not_a_container`] is where the difference is explained.
fn element_type(ty: &Type) -> Option<&Type> {
    let item = container_item(ty)?;
    // A container of containers has its element at the bottom, not one step down: the constraint
    // reads every level from the tokens and only the bottom is blank.
    Some(element_type(item).unwrap_or(item))
}

/// The item type of a sequence, or the value type of a map — one level, or [`None`].
///
/// A map's *key* type is skipped on purpose, for the reason the runtime walk skips it: a TOML
/// table's keys are strings whatever the map is keyed by, so nothing in the file is constrained
/// by it.
fn container_item(ty: &Type) -> Option<&Type> {
    match step(ty)? {
        // A wrapper is not the container being looked for; what it holds may be.
        Step::Through(inner) => container_item(inner),
        Step::Into(item) => Some(item),
    }
}

/// The type at the position the schema's reading of these tokens comes to rest.
///
/// The descent [`container_item`] makes, carried all the way down rather than stopping at the
/// first container: past every transparent wrapper, into every element position, until something
/// that is neither. `HashMap<String, Vec<Route>>` lands on `Route`, `Option<String>` lands on
/// `String`, and `u16` lands on itself — which is exactly where `terrace_config`'s own reading of
/// the type text stops, and so exactly where it has to be told what it cannot see.
fn described_position(ty: &Type) -> &Type {
    match step(ty) {
        Some(Step::Through(inner) | Step::Into(inner)) => described_position(inner),
        None => ty,
    }
}

/// One step of the walk over a type's tokens.
///
/// The two are distinguished because [`container_item`] needs to know which it took — the `Vec` in
/// an `Option<Vec<T>>` is the container the field holds and the `Option` is not — while
/// [`described_position`] treats them alike.
enum Step<'a> {
    /// A wrapper `serde` sees straight through: what it holds *is* the value.
    Through(&'a Type),
    /// A container's element position — a sequence's item, or a map's value.
    Into(&'a Type),
}

/// Past one wrapper, or into one container — or [`None`] for a type that is neither.
fn step(ty: &Type) -> Option<Step<'_>> {
    match ty {
        Type::Reference(reference) => Some(Step::Through(&reference.elem)),
        Type::Paren(paren) => Some(Step::Through(&paren.elem)),
        // What a macro-expanded type arrives wrapped in, and invisible in the source.
        Type::Group(group) => Some(Step::Through(&group.elem)),
        Type::Slice(slice) => Some(Step::Into(&slice.elem)),
        Type::Array(array) => Some(Step::Into(&array.elem)),
        Type::Path(path) if path.qself.is_none() => {
            let segment = path.path.segments.last()?;
            let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                return None;
            };
            // Lifetimes and const arguments are dropped, so `Cow<'a, T>` has one type argument
            // here and reads like every other single-argument wrapper.
            let args: Vec<&Type> = arguments
                .args
                .iter()
                .filter_map(|arg| match arg {
                    GenericArgument::Type(ty) => Some(ty),
                    _ => None,
                })
                .collect();
            match (segment.ident.to_string().as_str(), args.as_slice()) {
                (
                    "Option" | "Box" | "Arc" | "Rc" | "RefCell" | "Cell" | "Mutex" | "RwLock"
                    | "Cow",
                    [inner],
                ) => Some(Step::Through(inner)),
                ("Vec" | "VecDeque" | "HashSet" | "BTreeSet", [item]) => Some(Step::Into(item)),
                ("HashMap" | "BTreeMap", [_key, value]) => Some(Step::Into(value)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Options read from the container's own attributes.
struct Container {
    /// How `terrace_config` is named here. `::terrace_config` unless overridden.
    krate: Path,
    /// The container's `#[serde(rename_all = "…")]`, applied to every unrenamed field.
    rename_all: RenameRule,
    /// Whether `#[serde(default)]` on the container makes every field optional.
    field_default: bool,
    /// The container's `#[serde(deny_unknown_fields)]` — whether a key it did not declare is an
    /// error rather than something `serde` quietly ignores.
    deny_unknown_fields: bool,
}

impl Container {
    fn parse(input: &DeriveInput) -> syn::Result<Self> {
        let mut container = Self {
            krate: syn::parse_quote!(::terrace_config),
            rename_all: RenameRule::None,
            field_default: false,
            deny_unknown_fields: false,
        };

        for meta in attr_metas(&input.attrs, "serde")? {
            match &meta {
                Meta::Path(path) if path.is_ident("default") => container.field_default = true,
                Meta::Path(path) if path.is_ident("deny_unknown_fields") => {
                    container.deny_unknown_fields = true;
                }
                Meta::NameValue(nv) if nv.path.is_ident("default") => {
                    container.field_default = true;
                }
                Meta::NameValue(nv) if nv.path.is_ident("rename_all") => {
                    container.rename_all = RenameRule::parse(&string_value(&nv.value)?, &nv.value)?;
                }
                // `rename_all(deserialize = "…")`. Only the deserialising half can change which
                // key a configuration file has to spell.
                Meta::List(list) if list.path.is_ident("rename_all") => {
                    for inner in nested_metas(list)? {
                        if let Meta::NameValue(nv) = &inner
                            && nv.path.is_ident("deserialize")
                        {
                            container.rename_all =
                                RenameRule::parse(&string_value(&nv.value)?, &nv.value)?;
                        }
                    }
                }
                _ => {}
            }
        }

        for meta in attr_metas(&input.attrs, "config")? {
            match &meta {
                Meta::NameValue(nv) if nv.path.is_ident("crate") => {
                    let path = string_value(&nv.value)?;
                    container.krate = syn::parse_str(&path)
                        .map_err(|_| syn::Error::new_spanned(&nv.value, "not a crate path"))?;
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "unknown `#[config(...)]` option on a struct. The only one is \
                         `crate = \"…\"`.",
                    ));
                }
            }
        }

        Ok(container)
    }
}

/// Options read from one field's attributes.
///
/// A bag of independent flags, which is what an attribute list is. The states are not mutually
/// exclusive and there is no order between them, so the enum this lint asks for would have to
/// enumerate the product of all of them.
#[derive(Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per attribute; they are independent, not a state machine"
)]
struct FieldOpts {
    /// `#[serde(skip)]`, `#[serde(skip_deserializing)]`, or `#[config(skip)]`.
    skip: bool,
    /// `#[serde(flatten)]`.
    flatten: bool,
    /// `#[config(nested)]`.
    nested: bool,
    /// `#[config(secret)]`.
    secret: bool,
    /// `#[serde(rename = "…")]`, or its `deserialize` half.
    rename: Option<String>,
    /// `#[serde(default)]` or `#[serde(default = "…")]`.
    has_serde_default: bool,
    /// `#[config(note = "…")]`, the prose accompanying whatever value is observed.
    note: Option<String>,
    /// `#[config(values)]` or `#[config(values("…", "…"))]` — the fixed set of spellings the
    /// key accepts, and where it came from.
    values: Option<ValueList>,
    /// `#[config(range(...))]` — the interval the field's number has to fall in.
    range: Option<RangeOpts>,
    /// `#[config(element)]` or `#[config(element_values)]` — the field is a container, and this
    /// is what one element of it holds.
    element: Option<ElementKind>,
    /// Every `#[serde(alias = "…")]`, which are extra spellings the key also answers to.
    aliases: Vec<String>,
}

/// Which of the two things an element can be.
///
/// The same split as `nested` and `values` one level down, and it exists for the same reason those
/// are two attributes: a struct of named fields *has* keys, an enum of unit variants *is* a set of
/// values, and a derive looking at `Vec<Thing>` cannot tell which `Thing` is.
#[derive(Clone, PartialEq, Eq)]
enum ElementKind {
    /// The element derives `Describe`: it has keys of its own.
    Fields,
    /// The element is a fixed set of spellings.
    Choice(ValueList),
}

impl ElementKind {
    /// The attribute that asks for this, for an error message that can quote it back.
    fn attribute(&self) -> &'static str {
        match self {
            Self::Fields => "element",
            Self::Choice(_) => "element_values",
        }
    }
}

/// Where the fixed set of spellings a key accepts comes from.
///
/// [`Self::Trait`] is the derived path and the one to prefer: the variants come from the type, so
/// they cannot drift from it. [`Self::Literal`] exists because a trait cannot always be reached —
/// `impl Values for tracing::Level` is an orphan-rule error in every crate that would want to
/// write it, and this crate will not take a dependency on `tracing` to write it here.
#[derive(Clone, PartialEq, Eq)]
enum ValueList {
    /// The type's own `Values` implementation.
    Trait,
    /// Spellings the author listed, for a type no `Values` implementation can reach.
    ///
    /// Unverifiable by construction — nothing here reads the type's `Deserialize` — which is the
    /// standing `#[config(note = "…")]` already has, and is documented as such.
    Literal(Vec<String>),
}

impl ValueList {
    /// The `&'static [&'static str]` this reports, for a key or element whose type is `target`.
    fn tokens(&self, target: &Type, krate: &Path) -> TokenStream2 {
        match self {
            Self::Trait => quote! { <#target as #krate::schema::Values>::VARIANTS },
            Self::Literal(values) => quote! { &[#(#values),*] },
        }
    }
}

/// A `#[config(range(...))]` bound, in the form it was written.
///
/// The literal's own kind is kept rather than everything becoming an `f64`, because an `f64` holds
/// only the integers below 2^53 exactly and a bound that arrived rounded would be a *different*
/// number than the one the field takes.
#[derive(Clone, Copy)]
enum BoundLit {
    Integer(i64),
    Fractional(f64),
}

impl BoundLit {
    /// The expression reconstructing this as a `terrace_config::schema::Bound`.
    fn tokens(self, krate: &Path) -> TokenStream2 {
        match self {
            Self::Integer(value) => quote! { #krate::schema::Bound::Integer(#value) },
            Self::Fractional(value) => quote! { #krate::schema::Bound::Fractional(#value) },
        }
    }

    /// The bound as a real number, for checking that `min` does not sit above `max`.
    #[expect(
        clippy::cast_precision_loss,
        reason = "the rounding decides an ordering; the literal is emitted unchanged"
    )]
    fn approximate(self) -> f64 {
        match self {
            Self::Integer(value) => value as f64,
            Self::Fractional(value) => value,
        }
    }
}

/// Which end of a range a bound names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Edge {
    Lower,
    Upper,
}

impl Edge {
    /// The two spellings that set this end, for an error that can name both.
    fn spellings(self) -> &'static str {
        match self {
            Self::Lower => "`min` and `exclusive_min`",
            Self::Upper => "`max` and `exclusive_max`",
        }
    }
}

/// The bounds one field's `#[config(range(...))]` asks for.
#[derive(Default, Clone, Copy)]
struct RangeOpts {
    min: Option<BoundLit>,
    max: Option<BoundLit>,
    min_exclusive: bool,
    max_exclusive: bool,
}

impl RangeOpts {
    /// Read one `range(...)` list into this, rejecting an end that is already set.
    ///
    /// Additive across attributes, so `#[config(range(min = 0), range(max = 1))]` means what it
    /// looks like — but each end is settable once, because `min = 0, exclusive_min = 1` is two
    /// answers to one question and picking either would be the silent wrong answer this derive
    /// refuses everywhere else.
    fn extend(&mut self, list: &MetaList) -> syn::Result<()> {
        let bounds = nested_metas(list)?;
        if bounds.is_empty() {
            return Err(syn::Error::new_spanned(
                list,
                "`#[config(range(...))]` with no bounds says nothing about the value. It takes \
                 `min`, `max`, `exclusive_min` and `exclusive_max`, at least one of them.",
            ));
        }

        for meta in bounds {
            let Meta::NameValue(nv) = &meta else {
                return Err(syn::Error::new_spanned(
                    &meta,
                    "a range bound is written `name = number`, as in `min = 0` or `max = 1.0`.",
                ));
            };

            let (edge, exclusive) = if nv.path.is_ident("min") {
                (Edge::Lower, false)
            } else if nv.path.is_ident("exclusive_min") {
                (Edge::Lower, true)
            } else if nv.path.is_ident("max") {
                (Edge::Upper, false)
            } else if nv.path.is_ident("exclusive_max") {
                (Edge::Upper, true)
            } else {
                return Err(syn::Error::new_spanned(
                    &nv.path,
                    "unknown `#[config(range(...))]` bound. The four are `min`, `max`, \
                     `exclusive_min` and `exclusive_max`.",
                ));
            };

            let bound = bound_literal(&nv.value)?;
            let held = match edge {
                Edge::Lower => &mut self.min,
                Edge::Upper => &mut self.max,
            };
            if held.is_some() {
                return Err(syn::Error::new_spanned(
                    nv,
                    format!(
                        "{} are the same end of the range, and this field sets it twice.",
                        edge.spellings()
                    ),
                ));
            }
            *held = Some(bound);
            match edge {
                Edge::Lower => self.min_exclusive = exclusive,
                Edge::Upper => self.max_exclusive = exclusive,
            }
        }

        Ok(())
    }

    /// The two attribute names that set these ends, so an error can quote back what was written
    /// rather than a canonical spelling the field does not use.
    fn as_written(self) -> (&'static str, &'static str) {
        (
            if self.min_exclusive {
                "exclusive_min"
            } else {
                "min"
            },
            if self.max_exclusive {
                "exclusive_max"
            } else {
                "max"
            },
        )
    }

    /// The `terrace_config::schema::Bounds` literal these produce.
    fn tokens(self, krate: &Path) -> TokenStream2 {
        let min = optional(self.min.map(|bound| bound.tokens(krate)));
        let max = optional(self.max.map(|bound| bound.tokens(krate)));
        let (min_exclusive, max_exclusive) = (self.min_exclusive, self.max_exclusive);
        quote! {
            ::core::option::Option::Some(#krate::schema::Bounds {
                min: #min,
                max: #max,
                min_exclusive: #min_exclusive,
                max_exclusive: #max_exclusive,
            })
        }
    }
}

/// The number behind `name = 0` or `name = -1.5`.
///
/// A negation is a unary expression rather than part of the literal, so it is unwrapped here — an
/// attribute holding `min = -1` parses as `Expr::Unary` and would otherwise be rejected as "not a
/// literal", which names the wrong problem.
fn bound_literal(expr: &Expr) -> syn::Result<BoundLit> {
    let (negated, expr) = match expr {
        Expr::Unary(ExprUnary {
            op: UnOp::Neg(_),
            expr,
            ..
        }) => (true, &**expr),
        other => (false, other),
    };

    let Expr::Lit(ExprLit { lit, .. }) = expr else {
        return Err(syn::Error::new_spanned(
            expr,
            "a range bound is a numeric literal, as in `min = 0` or `max = 1.0`.",
        ));
    };

    match lit {
        Lit::Int(int) => {
            let value: i64 = int.base10_parse()?;
            Ok(BoundLit::Integer(if negated { -value } else { value }))
        }
        Lit::Float(float) => {
            let value: f64 = float.base10_parse()?;
            if !value.is_finite() {
                return Err(syn::Error::new_spanned(
                    float,
                    "a range bound has to be a finite number. JSON has no spelling for an \
                     infinity, so a schema could not carry this one.",
                ));
            }
            Ok(BoundLit::Fractional(if negated { -value } else { value }))
        }
        other => Err(syn::Error::new_spanned(
            other,
            "a range bound is a numeric literal, as in `min = 0` or `max = 1.0`.",
        )),
    }
}

/// `inner` as an `Option` expression, which is how every optional field of a `Leaf` is emitted.
fn optional(inner: Option<TokenStream2>) -> TokenStream2 {
    inner.map_or_else(
        || quote! { ::core::option::Option::None },
        |tokens| quote! { ::core::option::Option::Some(#tokens) },
    )
}

impl FieldOpts {
    fn parse(field: &Field) -> syn::Result<Self> {
        let mut opts = Self::default();

        for meta in attr_metas(&field.attrs, "serde")? {
            match &meta {
                Meta::Path(path) => {
                    if path.is_ident("skip") || path.is_ident("skip_deserializing") {
                        opts.skip = true;
                    } else if path.is_ident("flatten") {
                        opts.flatten = true;
                    } else if path.is_ident("default") {
                        opts.has_serde_default = true;
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("default") => opts.has_serde_default = true,
                // An alias is a second name the key answers to. Left out of the schema it is a
                // spelling that works and is documented nowhere — the same class of silent gap as
                // a wrong environment variable, pointing the other way.
                Meta::NameValue(nv) if nv.path.is_ident("alias") => {
                    opts.aliases.push(string_value(&nv.value)?);
                }
                Meta::NameValue(nv) if nv.path.is_ident("rename") => {
                    opts.rename = Some(string_value(&nv.value)?);
                }
                Meta::List(list) if list.path.is_ident("rename") => {
                    for inner in nested_metas(list)? {
                        if let Meta::NameValue(nv) = &inner
                            && nv.path.is_ident("deserialize")
                        {
                            opts.rename = Some(string_value(&nv.value)?);
                        }
                    }
                }
                _ => {}
            }
        }

        for meta in attr_metas(&field.attrs, "config")? {
            match &meta {
                Meta::Path(path) if path.is_ident("nested") => opts.nested = true,
                Meta::Path(path) if path.is_ident("secret") => opts.secret = true,
                Meta::Path(path) if path.is_ident("skip") => opts.skip = true,
                Meta::Path(path) if path.is_ident("values") => {
                    opts.set_values(ValueList::Trait, field)?;
                }
                Meta::Path(path) if path.is_ident("element") => {
                    opts.set_element(ElementKind::Fields, field)?;
                }
                Meta::Path(path) if path.is_ident("element_values") => {
                    opts.set_element(ElementKind::Choice(ValueList::Trait), field)?;
                }
                Meta::NameValue(nv) if nv.path.is_ident("note") => {
                    opts.note = Some(string_value(&nv.value)?);
                }
                Meta::List(list) if list.path.is_ident("range") => {
                    opts.range.get_or_insert_default().extend(list)?;
                }
                // The literal forms. `range` is the precedent: an option that takes arguments is
                // a `Meta::List`, and the bare `Meta::Path` spelling keeps the meaning it had.
                Meta::List(list) if list.path.is_ident("values") => {
                    let values = value_literals(list, "values")?;
                    opts.set_values(ValueList::Literal(values), field)?;
                }
                Meta::List(list) if list.path.is_ident("element_values") => {
                    let values = value_literals(list, "element_values")?;
                    opts.set_element(ElementKind::Choice(ValueList::Literal(values)), field)?;
                }
                // `default` was this attribute's name back when the prose *replaced* the observed
                // value rather than accompanying it. Named rather than left to the catch-all,
                // because the fix is a rename plus a reworded string, and a bare "unknown option"
                // would not say that.
                Meta::NameValue(nv) if nv.path.is_ident("default") => {
                    return Err(syn::Error::new_spanned(
                        nv,
                        "`#[config(default = \"…\")]` is now `#[config(note = \"…\")]`, and the \
                         prose no longer replaces the observed default — both are reported, so a \
                         note reading \"0 (permanent)\" should become just \"permanent\".",
                    ));
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "unknown `#[config(...)]` option. The field options are `nested`, \
                         `secret`, `skip`, `values`, `values(…)`, `element`, `element_values`, \
                         `element_values(…)`, `note = \"…\"`, and `range(…)`.",
                    ));
                }
            }
        }

        opts.check(field)?;
        Ok(opts)
    }

    /// The combinations that cannot mean anything, rejected with the fix rather than resolved by
    /// picking one.
    ///
    /// Separate from [`Self::parse`] because it is a separate job: reading an attribute list is
    /// about what serde and this derive spell, and this is about which of those claims can hold at
    /// once.
    fn check(&self, field: &Field) -> syn::Result<()> {
        if self.nested
            && (self.secret || self.note.is_some() || self.values.is_some() || self.range.is_some())
        {
            return Err(syn::Error::new_spanned(
                field,
                "`#[config(nested)]` describes a subtree, which has no single value to mark \
                 secret, to annotate, to enumerate, or to bound. Put `secret`, `note`, `values` \
                 or `range` on the leaf fields inside it.",
            ));
        }

        if self.range.is_some() && self.values.is_some() {
            return Err(syn::Error::new_spanned(
                field,
                "`#[config(values)]` says the key holds one of a fixed set of spellings, and \
                 `#[config(range(...))]` says it holds a number in an interval. A key is one or \
                 the other, and a field claiming both leaves the derive to pick.",
            ));
        }

        if let Some(element) = &self.element
            && self.range.is_some()
        {
            let attribute = element.attribute();
            return Err(syn::Error::new_spanned(
                field,
                format!(
                    "`#[config({attribute})]` and `#[config(range(...))]` describe the same \
                     position — what one element of the container holds. `{attribute}` says it \
                     is a type with a shape of its own, `range` says it is a number in an \
                     interval, and an element is one or the other. A bound on a field *inside* a \
                     described element belongs on that element type's own field."
                ),
            ));
        }

        if let Some(range) = self.range
            && let (Some(min), Some(max)) = (range.min, range.max)
            && min.approximate() > max.approximate()
        {
            let (lower, upper) = range.as_written();
            return Err(syn::Error::new_spanned(
                field,
                format!(
                    "`{lower}` is above `{upper}`, so this range accepts no value at all. A \
                     schema saying that rejects every configuration, including the ones that load."
                ),
            ));
        }

        if self.element.is_some() && (self.nested || self.values.is_some()) {
            return Err(syn::Error::new_spanned(
                field,
                "`#[config(nested)]` and `#[config(values)]` describe the field's own type, and \
                 `#[config(element)]` describes what a container of that type holds — so a field \
                 carrying both says its value is two different things. A container's keys are its \
                 elements' keys and belong in the element schema; drop `nested` or `values`.",
            ));
        }

        Ok(())
    }

    /// Record what shape one element of this field's container has.
    ///
    /// Repeating the same attribute is harmless; asking for both is not. An element is a type with
    /// keys of its own or an enum of unit variants, never both, and a field claiming both leaves
    /// the derive to pick — which is the silent wrong answer this crate refuses everywhere else.
    fn set_element(&mut self, kind: ElementKind, field: &Field) -> syn::Result<()> {
        match &self.element {
            Some(held) if held.attribute() != kind.attribute() => Err(syn::Error::new_spanned(
                field,
                "`#[config(element)]` and `#[config(element_values)]` describe the same element \
                 two ways. It is either a type with keys of its own, which `element` reports, or \
                 an enum of unit variants, which `element_values` reports.",
            )),
            Some(held) if *held != kind => Err(syn::Error::new_spanned(
                field,
                "`#[config(element_values)]` names the spellings one element accepts twice, and \
                 the two answers differ. Bare, it reports the element type's own `Values` \
                 implementation; with a list, it reports the list.",
            )),
            _ => {
                self.element = Some(kind);
                Ok(())
            }
        }
    }

    /// Record where the values this key accepts come from.
    ///
    /// Repeating the attribute says nothing new, which is not the same as saying two things: bare
    /// `values` reports the field type's own `Values` implementation and a list reports the list,
    /// and a field asking for both leaves the derive to pick.
    fn set_values(&mut self, source: ValueList, field: &Field) -> syn::Result<()> {
        if self.values.as_ref().is_some_and(|held| *held != source) {
            return Err(syn::Error::new_spanned(
                field,
                "`#[config(values)]` names the values this key accepts twice, and the two \
                 answers differ. Bare, it reports the field type's own `Values` implementation; \
                 with a list, it reports the list.",
            ));
        }
        self.values = Some(source);
        Ok(())
    }

    /// The key segment this field contributes, which must be the one serde will look for.
    fn name(&self, field: &Field, container: &Container) -> syn::Result<String> {
        if let Some(rename) = &self.rename {
            return Ok(rename.clone());
        }
        let ident = field
            .ident
            .as_ref()
            .ok_or_else(|| syn::Error::new_spanned(field, "expected a named field"))?;
        // `r#type` is spelled `type` in every serialisation format.
        let raw = ident.to_string();
        let name = raw.strip_prefix("r#").unwrap_or(&raw);
        Ok(container.rename_all.apply(name))
    }
}

/// serde's `rename_all` rules, reimplemented because reading the container attribute is the only
/// way the generated key paths can agree with the ones serde looks for.
#[derive(Clone, Copy)]
enum RenameRule {
    None,
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

impl RenameRule {
    fn parse(name: &str, span: &Expr) -> syn::Result<Self> {
        Ok(match name {
            "lowercase" => Self::Lower,
            "UPPERCASE" => Self::Upper,
            "PascalCase" => Self::Pascal,
            "camelCase" => Self::Camel,
            "snake_case" => Self::Snake,
            "SCREAMING_SNAKE_CASE" => Self::ScreamingSnake,
            "kebab-case" => Self::Kebab,
            "SCREAMING-KEBAB-CASE" => Self::ScreamingKebab,
            other => {
                return Err(syn::Error::new_spanned(
                    span,
                    format!("`{other}` is not a serde rename rule"),
                ));
            }
        })
    }

    /// Variant names are `PascalCase` by convention, so the same rule means something different
    /// here than it does for a field — `snake_case` has to *insert* the underscores that
    /// [`Self::apply`] merely keeps.
    ///
    /// Reimplemented rather than approximated, because a value the table spells differently from
    /// the one serde accepts is a value nobody can set.
    fn apply_to_variant(self, variant: &str) -> String {
        match self {
            Self::None | Self::Pascal => variant.to_owned(),
            Self::Lower => variant.to_ascii_lowercase(),
            Self::Upper => variant.to_ascii_uppercase(),
            Self::Camel => {
                let mut chars = variant.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            }
            Self::Snake => snake(variant),
            Self::ScreamingSnake => snake(variant).to_ascii_uppercase(),
            Self::Kebab => snake(variant).replace('_', "-"),
            Self::ScreamingKebab => snake(variant).to_ascii_uppercase().replace('_', "-"),
        }
    }

    /// Field names are `snake_case` by convention, which is what serde assumes here too.
    fn apply(self, field: &str) -> String {
        match self {
            Self::None | Self::Snake => field.to_owned(),
            Self::Lower => field.to_ascii_lowercase(),
            Self::Upper | Self::ScreamingSnake => field.to_ascii_uppercase(),
            Self::Pascal => pascal(field),
            Self::Camel => {
                let pascal = pascal(field);
                let mut chars = pascal.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
                    None => pascal,
                }
            }
            Self::Kebab => field.replace('_', "-"),
            Self::ScreamingKebab => field.to_ascii_uppercase().replace('_', "-"),
        }
    }
}

/// `PascalCase` to `snake_case`, the way serde converts a variant name.
fn snake(variant: &str) -> String {
    let mut out = String::with_capacity(variant.len());
    for (index, character) in variant.char_indices() {
        if character.is_uppercase() && index > 0 {
            out.push('_');
        }
        out.extend(character.to_lowercase());
    }
    out
}

fn pascal(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    for segment in field.split('_') {
        let mut chars = segment.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// Every `#[name(...)]` argument across every `#[name(...)]` attribute on an item.
///
/// Unknown arguments are returned rather than rejected: this parses serde's attributes as well
/// as its own, and rejecting an option serde understands would make the derive a reason not to
/// upgrade serde.
fn attr_metas(attrs: &[Attribute], name: &str) -> syn::Result<Vec<Meta>> {
    let mut metas = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident(name) {
            continue;
        }
        metas.extend(attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?);
    }
    Ok(metas)
}

/// The arguments of a nested `key(...)` form, such as serde's `rename(deserialize = "…")`.
fn nested_metas(list: &syn::MetaList) -> syn::Result<Vec<Meta>> {
    Ok(list
        .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?
        .into_iter()
        .collect())
}

/// The string behind `key = "value"`.
fn string_value(expr: &Expr) -> syn::Result<String> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(lit), ..
        }) => Ok(lit.value()),
        other => Err(syn::Error::new_spanned(
            other,
            "expected a string literal, as in `key = \"value\"`",
        )),
    }
}

/// The `///` comments on an item, dedented and joined.
///
/// Line structure is preserved rather than collapsed to one line: the JSON output is consumed by
/// something that decides its own layout, and a renderer can always collapse what it is given
/// whereas it cannot recover what was thrown away.
fn doc_comment(attrs: &[Attribute]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let Meta::NameValue(nv) = &attr.meta else {
            continue;
        };
        let Ok(text) = string_value(&nv.value) else {
            continue;
        };
        // `/// Text` yields `" Text"`. One space, and one only: further indentation is the
        // author's, and a code block inside a doc comment depends on it.
        for line in text.split('\n') {
            lines.push(line.strip_prefix(' ').unwrap_or(line).to_owned());
        }
    }

    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// A field's type as an operator would read it.
///
/// The token text, tidied: `Vec < String >` is what a `TokenStream` prints and `Vec<String>` is
/// what anyone documenting a configuration wants to see. Nothing is resolved — a type alias
/// prints as its alias and `SecretString` prints as itself — because a derive has no types, only
/// tokens. That is honest and it is what the field says; inventing a type language that claimed
/// otherwise would be worse than printing what is written.
fn type_text(ty: &Type) -> String {
    let mut text = quote!(#ty).to_string();
    // Order matters: the path separator first, so its spaces are gone before the angle brackets
    // are closed up around it.
    for (from, to) in [
        (" :: ", "::"),
        (":: ", "::"),
        (" ::", "::"),
        (" <", "<"),
        ("< ", "<"),
        (" >", ">"),
        ("> ", ">"),
        (" ,", ","),
        ("& ", "&"),
        (" ;", ";"),
    ] {
        while text.contains(from) {
            text = text.replace(from, to);
        }
    }
    text
}

/// Whether a type is syntactically `Option<_>`.
///
/// Syntactically, because a derive has no types — only tokens. `std::option::Option<T>` and a
/// type alias for it are the known limits: the first is matched by its last segment, the second
/// is indistinguishable from any other single identifier and is treated as required. An alias
/// for `Option` in a configuration struct is rare enough to leave to `#[serde(default)]`, which
/// the field would carry anyway.
fn is_option(ty: &Type) -> bool {
    unwrap_option(ty).is_some()
}

/// The `T` in `Option<T>`.
fn unwrap_option(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else { return None };
    if path.qself.is_some() {
        return None;
    }
    let segment = path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    if args.args.len() != 1 {
        return None;
    }
    match args.args.first()? {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Container, DeriveInput, KNOWN_LEAVES, RenameRule, Type, doc_comment, expand, is_option,
        quote, type_text, unwrap_option,
    };

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

    /// A struct with no fields at all is legal and describes nothing — an empty configuration
    /// section is a real thing, and refusing it would be refusing a shape serde accepts.
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
        let body =
            generated("struct S { #[config(range(min = 0.0, exclusive_max = 1.0))] rate: f32 }");
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

    /// A negation is a unary expression rather than part of the literal, so it has to be unwrapped
    /// — otherwise the error names the wrong problem.
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
    /// answer for a struct that said nothing — and the feature never becomes a default.
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
        let body = generated(
            "struct S { #[serde(rename(serialize = \"out\", deserialize = \"in\"))] a: u8 }",
        );
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
        assert!(
            !generated("struct S { #[serde(skip_deserializing)] a: u8 }").contains("sink . leaf")
        );
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

    /// `#[doc = "…"]` is what `///` desugars to, so both have to work — a macro-generated struct
    /// carries the explicit form.
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
            // The `Option` is stripped — `required` already carries that.
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
            body.contains(
                "Element :: Fields (< Route as :: terrace_config :: schema :: Describe >"
            ),
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

    /// Guessing would be silent and wrong — a schema saying `routes` is an object because its
    /// element is a struct describes a file nobody can write.
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
            "nested",
            "skip",
            "range(min = 0.0, max = 1.0)",
        ] {
            let input = format!("struct S {{ #[config({attribute})] a: Level }}");
            let parsed: DeriveInput = syn::parse_str(&input).expect("test input is valid Rust");
            assert!(expand(&parsed).is_ok(), "{attribute}");
        }
        for attribute in ["element", "element_values", r#"element_values("a", "b")"#] {
            let input = format!("struct S {{ #[config({attribute})] a: Vec<Level> }}");
            let parsed: DeriveInput = syn::parse_str(&input).expect("test input is valid Rust");
            assert!(expand(&parsed).is_ok(), "{attribute}");
        }
    }

    /// A tuple, a qualified path and an unrecognised generic publish nothing either — and none of
    /// the five attributes is an answer for them, so an error naming them would be a dead end.
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
        let body =
            generated(r#"struct S { #[config(values("trace", "debug"))] a: tracing::Level }"#);
        assert!(
            body.contains(
                r#"values : :: core :: option :: Option :: Some (& ["trace" , "debug"])"#
            ),
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
        let error = rejected(r#"struct S { #[config(values, values("a"))] x: Level }"#);
        assert!(
            error.contains("names the values this key accepts twice"),
            "{error}"
        );

        let error =
            rejected(r#"struct S { #[config(element_values, element_values("a"))] x: Vec<L> }"#);
        assert!(
            error.contains("names the spellings one element accepts twice"),
            "{error}"
        );
    }

    /// Repeating one attribute says nothing new, which is not the same as saying two things.
    #[test]
    fn repeating_the_same_value_list_is_harmless() {
        let body =
            generated(r#"struct S { #[config(values("a"))] #[config(values("a"))] x: Level }"#);
        assert!(body.contains(r#"& ["a"]"#), "{body}");
    }

    /// A list says the key holds one of a fixed set of spellings, which is the claim `range`
    /// collides with however the set was arrived at.
    #[test]
    fn a_literal_value_list_still_collides_with_a_range() {
        let error = rejected(r#"struct S { #[config(values("a"), range(min = 0))] x: Level }"#);
        assert!(error.contains("fixed set of spellings"), "{error}");
    }

    /// A container with no element attribute is the case that has to keep generating exactly what
    /// it generated before.
    #[test]
    fn a_container_that_says_nothing_is_still_a_plain_leaf() {
        let body = generated("struct S { a: Vec<String> }");
        assert!(body.contains("sink . leaf"), "{body}");
        assert!(!body.contains("repeated"), "{body}");
    }
}
