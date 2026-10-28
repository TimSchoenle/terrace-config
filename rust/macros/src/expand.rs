//! The token streams the derive emits: one `Describe` or `Values` impl per input type, and one
//! `Leaf` per field.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Attribute, DeriveInput, Field, Fields, Meta, Path, Token};

use crate::attrs::{ReloadClass, attr_metas, doc_comment, nested_metas, string_value};
use crate::container::Container;
use crate::field::{ElementKind, FieldOpts, optional};
use crate::types::{
    describes_something, element_type, is_option, not_a_container, type_text, unwrap_option,
};

/// A struct's keys.
pub(crate) fn expand_struct(
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
    let body = within_reload(body, container.reload, krate);

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
pub(crate) fn expand_enum(
    input: &DeriveInput,
    container: &Container,
    data: &syn::DataEnum,
) -> syn::Result<TokenStream2> {
    if container.reload.is_some() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "`#[config(reload = \"…\")]` describes keys, and an enum deriving `Describe` is the \
             set of values one key accepts rather than a set of keys. Put it on the field holding \
             the enum, or on the struct containing that field.",
        ));
    }

    // Before the variants are read, because with a conversion in the way they are not what a
    // configuration file may hold and there is nothing to report.
    if let Some(attribute) = &container.foreign_deserialize {
        return Err(syn::Error::new_spanned(
            &input.ident,
            format!(
                "`Describe` on an enum reports its variants as the values a key accepts, and \
                 `#[serde({attribute} = \"…\")]` says those variants are not what `Deserialize` \
                 matches — it reads something else and converts. A case-insensitive `FromStr` is \
                 the conversion that turns up, and it accepts spellings no variant is named \
                 after, so publishing the variant list would be a schema rejecting a file the \
                 loader takes. Describe such a type from the field that holds it, with \
                 `#[config(values(\"…\", \"…\"))]` naming what the conversion really accepts."
            ),
        ));
    }

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
pub(crate) struct VariantOpts {
    /// `#[serde(skip)]` or `#[serde(skip_deserializing)]`.
    skip: bool,
    /// `#[serde(rename = "…")]`, or its `deserialize` half.
    rename: Option<String>,
}

impl VariantOpts {
    pub(crate) fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
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
pub(crate) fn field_tokens(field: &Field, container: &Container) -> syn::Result<TokenStream2> {
    let opts = FieldOpts::parse(field)?;
    if opts.skip {
        return Ok(TokenStream2::new());
    }
    let reload = opts.reload;
    let body = field_body(field, container, &opts)?;
    Ok(within_reload(body, reload, &container.krate))
}

/// `body`, inside a `Sink::reload` scope when a `#[config(reload = "…")]` asked for one.
pub(crate) fn within_reload(
    body: TokenStream2,
    class: Option<ReloadClass>,
    krate: &Path,
) -> TokenStream2 {
    let Some(class) = class else {
        return body;
    };
    let class = class.tokens(krate);
    quote! { sink.reload(#class, |sink| { #body }); }
}

/// A field's keys, before any `#[config(reload = "…")]` scope is put around them.
pub(crate) fn field_body(
    field: &Field,
    container: &Container,
    opts: &FieldOpts,
) -> syn::Result<TokenStream2> {
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
    let item = element_type(bare).ok_or_else(|| not_a_container(field, element.attribute()))?;
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
