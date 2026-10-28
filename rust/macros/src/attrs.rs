//! Small readers over `syn` attributes and expressions shared by the container and field parsers,
//! and the attribute values both of them accept.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, ExprLit, Lit, Meta, Path, Token, Type};

/// Every `#[name(...)]` argument across every `#[name(...)]` attribute on an item.
///
/// Unknown arguments are returned rather than rejected: this parses serde's attributes as well
/// as its own, and rejecting an option serde understands would make the derive a reason not to
/// upgrade serde.
pub(crate) fn attr_metas(attrs: &[Attribute], name: &str) -> syn::Result<Vec<Meta>> {
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
pub(crate) fn nested_metas(list: &syn::MetaList) -> syn::Result<Vec<Meta>> {
    Ok(list
        .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?
        .into_iter()
        .collect())
}

/// The type behind `values_from = "LevelDef"`.
///
/// A string rather than a bare path, for the reason `crate = "…"` is one: this attribute
/// vocabulary spells a path as a string everywhere, and serde's own `with` and `default` do the
/// same. The span stays on the literal, so an error points at the attribute rather than at the
/// field.
pub(crate) fn named_type(expr: &Expr, attribute: &str) -> syn::Result<Type> {
    let path = string_value(expr)?;
    syn::parse_str(&path).map_err(|_| {
        syn::Error::new_spanned(
            expr,
            format!(
                "`#[config({attribute} = \"…\")]` names the type whose `Values` implementation \
                 says what this key accepts, as in `{attribute} = \"LevelDef\"`. \
                 `{path}` is not a type."
            ),
        )
    })
}

/// An attribute's own name, for an error that quotes back the spelling that was written rather
/// than a canonical one the source does not use.
pub(crate) fn path_ident(path: &Path) -> String {
    path.segments
        .last()
        .map_or_else(String::new, |segment| segment.ident.to_string())
}

/// The string behind `key = "value"`.
pub(crate) fn string_value(expr: &Expr) -> syn::Result<String> {
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
pub(crate) fn doc_comment(attrs: &[Attribute]) -> String {
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

/// Whether a rebuild applies a change, as `#[config(reload = "…")]` spells it.
#[derive(Clone, Copy)]
pub(crate) enum ReloadClass {
    /// `"live"`.
    Live,
    /// `"restart"`.
    Restart,
}

impl ReloadClass {
    pub(crate) fn parse(value: &Expr) -> syn::Result<Self> {
        match string_value(value)?.as_str() {
            "live" => Ok(Self::Live),
            "restart" => Ok(Self::Restart),
            other => Err(syn::Error::new_spanned(
                value,
                format!(
                    "`reload = \"{other}\"` names no reload class. It is `\"live\"`, for a value \
                     a rebuild applies, or `\"restart\"`, for one only a process start applies."
                ),
            )),
        }
    }

    pub(crate) fn tokens(self, krate: &Path) -> TokenStream2 {
        match self {
            Self::Live => quote! { #krate::schema::Reload::Live },
            Self::Restart => quote! { #krate::schema::Reload::Restart },
        }
    }
}
