//! The container-level attributes: `#[serde(rename_all, default, deny_unknown_fields)]` and
//! `#[config(...)]` on the struct or enum itself.

use syn::{DeriveInput, Expr, Meta, Path};

use crate::attrs::{attr_metas, nested_metas, path_ident, string_value};
use crate::expand::ReloadClass;

/// Options read from the container's own attributes.
pub(crate) struct Container {
    /// How `terrace_config` is named here. `::terrace_config` unless overridden.
    pub(crate) krate: Path,
    /// The container's `#[serde(rename_all = "…")]`, applied to every unrenamed field.
    pub(crate) rename_all: RenameRule,
    /// Whether `#[serde(default)]` on the container makes every field optional.
    pub(crate) field_default: bool,
    /// The container's `#[serde(deny_unknown_fields)]` — whether a key it did not declare is an
    /// error rather than something `serde` quietly ignores.
    pub(crate) deny_unknown_fields: bool,
    /// The container's `#[serde(try_from = "…")]` or `#[serde(from = "…")]`, named so an error can
    /// quote back the one that was written.
    ///
    /// Both mean the derived `Deserialize` reads a *different* type and converts, so nothing this
    /// derive can see about the container describes what a file may hold. `remote` is deliberately
    /// not one of them: a remote mirror's variants are matched exactly, which is what makes it the
    /// answer rather than the problem.
    pub(crate) foreign_deserialize: Option<String>,
    /// The container's `#[config(reload = "…")]`, covering every field it describes.
    pub(crate) reload: Option<ReloadClass>,
}

impl Container {
    pub(crate) fn parse(input: &DeriveInput) -> syn::Result<Self> {
        let mut container = Self {
            krate: syn::parse_quote!(::terrace_config),
            rename_all: RenameRule::None,
            field_default: false,
            deny_unknown_fields: false,
            foreign_deserialize: None,
            reload: None,
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
                Meta::NameValue(nv) if nv.path.is_ident("try_from") || nv.path.is_ident("from") => {
                    container.foreign_deserialize = Some(path_ident(&nv.path));
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
                Meta::NameValue(nv) if nv.path.is_ident("reload") => {
                    container.reload = Some(ReloadClass::parse(&nv.value)?);
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "unknown `#[config(...)]` option on a struct. The options are \
                         `crate = \"…\"` and `reload = \"…\"`.",
                    ));
                }
            }
        }

        Ok(container)
    }
}

/// serde's `rename_all` rules, reimplemented because reading the container attribute is the only
/// way the generated key paths can agree with the ones serde looks for.
#[derive(Clone, Copy)]
pub(crate) enum RenameRule {
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
    pub(crate) fn parse(name: &str, span: &Expr) -> syn::Result<Self> {
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
    pub(crate) fn apply_to_variant(self, variant: &str) -> String {
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
    pub(crate) fn apply(self, field: &str) -> String {
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
pub(crate) fn snake(variant: &str) -> String {
    let mut out = String::with_capacity(variant.len());
    for (index, character) in variant.char_indices() {
        if character.is_uppercase() && index > 0 {
            out.push('_');
        }
        out.extend(character.to_lowercase());
    }
    out
}

pub(crate) fn pascal(field: &str) -> String {
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
