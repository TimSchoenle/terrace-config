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
    Attribute, Data, DeriveInput, Expr, ExprLit, Field, Fields, GenericArgument, Lit, Meta, Path,
    PathArguments, Token, Type, parse_macro_input,
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
/// # `#[config(...)]`
///
/// | Attribute | Effect |
/// |---|---|
/// | `#[config(nested)]` | Recurse into the field's type instead of treating it as a leaf |
/// | `#[config(secret)]` | Render the default as `<redacted>`, and mark the key in the output |
/// | `#[config(note = "…")]` | Annotate the observed default with prose |
/// | `#[config(values)]` | Report the field type's variants as the values the key accepts |
/// | `#[config(element)]` | Report the shape of one element of a container-typed key |
/// | `#[config(element_values)]` | Report the values one element of a container-typed key accepts |
/// | `#[config(skip)]` | Omit the key from the schema without affecting deserialisation |
/// | `#[config(crate = "…")]` | Name the `terrace_config` crate, if it was renamed |
///
/// `nested` is opt-in because no macro can tell a `PathBuf` from a nested config struct by
/// looking at the type: both are one identifier and a module path. Guessing would mean either
/// bare identifiers silently becoming leaves, or a bound on types that cannot satisfy it.
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
    let values = if opts.values {
        quote! { ::core::option::Option::Some(<#bare as #krate::schema::Values>::VARIANTS) }
    } else {
        quote! { ::core::option::Option::None }
    };
    let aliases = &opts.aliases;

    let leaf = quote! {
        #krate::schema::Leaf {
            name: #name,
            docs: #docs,
            ty: ::core::option::Option::Some(#ty_text),
            values: #values,
            aliases: &[#(#aliases),*],
            note: #note,
            required: #required,
            secret: #secret,
        }
    };

    // A container-typed key is still one key. What changes is that the element the type token
    // cannot name is reported alongside it, and lands nested inside the key's constraint.
    let Some(element) = opts.element else {
        return Ok(quote! { sink.leaf(#leaf); });
    };
    let item = element_type(bare).ok_or_else(|| not_a_container(field, element))?;
    let reported = match element {
        ElementKind::Fields => {
            quote! { #krate::schema::Element::Fields(<#item as #krate::schema::Describe>::describe) }
        }
        ElementKind::Choice => {
            quote! { #krate::schema::Element::Choice(<#item as #krate::schema::Values>::VARIANTS) }
        }
    };
    Ok(quote! { sink.repeated(#leaf, #reported); })
}

/// The error for `#[config(element)]` on a field this derive cannot find a container in.
///
/// Named rather than guessed at, because the guess would be silent and wrong: a schema saying
/// `routes` is an object because its element is a struct describes a file nobody can write. The
/// alias case is called out because it is the one that looks like a bug in the derive — the type
/// *is* a container, and a derive cannot see through a name to know it.
fn not_a_container(field: &Field, element: ElementKind) -> syn::Error {
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
    match ty {
        Type::Reference(reference) => container_item(&reference.elem),
        Type::Paren(paren) => container_item(&paren.elem),
        // What a macro-expanded type arrives wrapped in, and invisible in the source.
        Type::Group(group) => container_item(&group.elem),
        Type::Slice(slice) => Some(&slice.elem),
        Type::Array(array) => Some(&array.elem),
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
                ) => container_item(inner),
                ("Vec" | "VecDeque" | "HashSet" | "BTreeSet", [item]) => Some(item),
                ("HashMap" | "BTreeMap", [_key, value]) => Some(value),
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
}

impl Container {
    fn parse(input: &DeriveInput) -> syn::Result<Self> {
        let mut container = Self {
            krate: syn::parse_quote!(::terrace_config),
            rename_all: RenameRule::None,
            field_default: false,
        };

        for meta in attr_metas(&input.attrs, "serde")? {
            match &meta {
                Meta::Path(path) if path.is_ident("default") => container.field_default = true,
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
    /// `#[config(values)]` — the field's type is an enum whose variants are the accepted values.
    values: bool,
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
#[derive(Clone, Copy, PartialEq, Eq)]
enum ElementKind {
    /// The element derives `Describe`: it has keys of its own.
    Fields,
    /// The element derives `Values`: it is a fixed set of spellings.
    Choice,
}

impl ElementKind {
    /// The attribute that asks for this, for an error message that can quote it back.
    fn attribute(self) -> &'static str {
        match self {
            Self::Fields => "element",
            Self::Choice => "element_values",
        }
    }
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
                Meta::Path(path) if path.is_ident("values") => opts.values = true,
                Meta::Path(path) if path.is_ident("element") => {
                    opts.set_element(ElementKind::Fields, field)?;
                }
                Meta::Path(path) if path.is_ident("element_values") => {
                    opts.set_element(ElementKind::Choice, field)?;
                }
                Meta::NameValue(nv) if nv.path.is_ident("note") => {
                    opts.note = Some(string_value(&nv.value)?);
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
                         `secret`, `skip`, `values`, `element`, `element_values`, and \
                         `note = \"…\"`.",
                    ));
                }
            }
        }

        if opts.nested && (opts.secret || opts.note.is_some() || opts.values) {
            return Err(syn::Error::new_spanned(
                field,
                "`#[config(nested)]` describes a subtree, which has no single value to mark \
                 secret, to annotate, or to enumerate. Put `secret`, `note` or `values` on the \
                 leaf fields inside it.",
            ));
        }

        if opts.element.is_some() && (opts.nested || opts.values) {
            return Err(syn::Error::new_spanned(
                field,
                "`#[config(nested)]` and `#[config(values)]` describe the field's own type, and \
                 `#[config(element)]` describes what a container of that type holds — so a field \
                 carrying both says its value is two different things. A container's keys are its \
                 elements' keys and belong in the element schema; drop `nested` or `values`.",
            ));
        }

        Ok(opts)
    }

    /// Record what shape one element of this field's container has.
    ///
    /// Repeating the same attribute is harmless; asking for both is not. An element is a type with
    /// keys of its own or an enum of unit variants, never both, and a field claiming both leaves
    /// the derive to pick — which is the silent wrong answer this crate refuses everywhere else.
    fn set_element(&mut self, kind: ElementKind, field: &Field) -> syn::Result<()> {
        if self.element.is_some_and(|held| held != kind) {
            return Err(syn::Error::new_spanned(
                field,
                "`#[config(element)]` and `#[config(element_values)]` describe the same element \
                 two ways. It is either a type with keys of its own, which `element` reports, or \
                 an enum of unit variants, which `element_values` reports.",
            ));
        }
        self.element = Some(kind);
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
        Container, DeriveInput, RenameRule, Type, doc_comment, expand, is_option, quote, type_text,
        unwrap_option,
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

    /// A container with no element attribute is the case that has to keep generating exactly what
    /// it generated before.
    #[test]
    fn a_container_that_says_nothing_is_still_a_plain_leaf() {
        let body = generated("struct S { a: Vec<Route> }");
        assert!(body.contains("sink . leaf"), "{body}");
        assert!(!body.contains("repeated"), "{body}");
    }
}
