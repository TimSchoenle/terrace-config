//! The field-level attributes: parsing `#[serde(...)]` and `#[config(...)]` on one field into the
//! options that shape its `Leaf`.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Expr, ExprLit, ExprUnary, Field, Lit, Meta, MetaList, Path, Token, Type, UnOp};

use crate::attrs::{attr_metas, named_type, nested_metas, path_ident, string_value};
use crate::container::Container;
use crate::expand::{ReloadClass, optional};
use crate::types::type_text;

/// The spellings behind `values("trace", "debug")`, checked for the two ways such a list says
/// nothing.
///
/// Whether the list *matches the type* is not checkable here and is not checked: nothing in a
/// derive reads a foreign type's `Deserialize`. What is checkable is that the assertion is well
/// formed — an empty list claims the key accepts no value at all, and a repeated spelling is a
/// list that was edited in two places and would reach the schema's `enum` twice.
pub(crate) fn value_literals(list: &MetaList, attribute: &str) -> syn::Result<Vec<String>> {
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
pub(crate) struct FieldOpts {
    /// `#[serde(skip)]`, `#[serde(skip_deserializing)]`, or `#[config(skip)]`.
    pub(crate) skip: bool,
    /// `#[serde(flatten)]`.
    pub(crate) flatten: bool,
    /// `#[config(nested)]`.
    pub(crate) nested: bool,
    /// `#[config(secret)]`.
    pub(crate) secret: bool,
    /// `#[serde(rename = "…")]`, or its `deserialize` half.
    rename: Option<String>,
    /// `#[serde(default)]` or `#[serde(default = "…")]`.
    pub(crate) has_serde_default: bool,
    /// `#[config(note = "…")]`, the prose accompanying whatever value is observed.
    pub(crate) note: Option<String>,
    /// `#[config(values)]` or `#[config(values("…", "…"))]` — the fixed set of spellings the
    /// key accepts, and where it came from.
    pub(crate) values: Option<ValueList>,
    /// `#[config(range(...))]` — the interval the field's number has to fall in.
    pub(crate) range: Option<RangeOpts>,
    /// `#[config(element)]` or `#[config(element_values)]` — the field is a container, and this
    /// is what one element of it holds.
    pub(crate) element: Option<ElementKind>,
    /// Every `#[serde(alias = "…")]`, which are extra spellings the key also answers to.
    pub(crate) aliases: Vec<String>,
    /// `#[serde(with = "…")]` or `#[serde(deserialize_with = "…")]`, whichever was written.
    ///
    /// Serde's two *field* attributes that replace this field's `Deserialize`, so the variants a
    /// type derives are no longer the spellings a file may hold. `try_from` and `from` do the same
    /// thing one level up — they are container attributes, and [`expand_enum`] refuses there.
    ///
    /// Only the name is kept. What these name is a path, and a derive cannot follow one: that a
    /// custom parse exists is the whole of what is knowable here, and it is enough to know the
    /// derived list is not it.
    custom_deserialize: Option<String>,
    /// `#[config(reload = "…")]`, covering this field's key or everything under it.
    pub(crate) reload: Option<ReloadClass>,
}

/// Which of the two things an element can be.
///
/// The same split as `nested` and `values` one level down, and it exists for the same reason those
/// are two attributes: a struct of named fields *has* keys, an enum of unit variants *is* a set of
/// values, and a derive looking at `Vec<Thing>` cannot tell which `Thing` is.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ElementKind {
    /// The element derives `Describe`: it has keys of its own.
    Fields,
    /// The element is a fixed set of spellings.
    Choice(ValueList),
}

impl ElementKind {
    /// The attribute that asks for this, for an error message that can quote it back.
    pub(crate) fn attribute(&self) -> &'static str {
        match self {
            Self::Fields => "element",
            Self::Choice(_) => "element_values",
        }
    }

    /// Where a choice's spellings come from, for an error that can name both of two answers.
    ///
    /// [`ValueList::Trait`] for [`Self::Fields`], which has no spellings of its own — the two are
    /// only ever compared after [`Self::attribute`] has already told them apart.
    pub(crate) fn value_list(&self) -> &ValueList {
        match self {
            Self::Fields => &ValueList::Trait,
            Self::Choice(source) => source,
        }
    }
}

/// Where the fixed set of spellings a key accepts comes from.
///
/// [`Self::Trait`] is the derived path and the one to prefer: the variants come from the type, so
/// they cannot drift from it. [`Self::Literal`] exists because a trait cannot always be reached —
/// `impl Values for tracing::Level` is an orphan-rule error in every crate that would want to
/// write it, and this crate will not take a dependency on `tracing` to write it here.
#[derive(Clone)]
pub(crate) enum ValueList {
    /// The type's own `Values` implementation.
    Trait,
    /// Another type's, named because the field's own has none.
    ///
    /// A `#[serde(remote = "…")]` mirror is what this exists for: serde matches the mirror's
    /// variants exactly, `Describe` on it reports those same variants under the same
    /// `rename_all`, and the two cannot drift because they are one declaration. Still checked by
    /// the compiler — the named type has to implement `Values` — which is what separates it from
    /// a list.
    ///
    /// Boxed because a `syn::Type` is an order of magnitude larger than either other variant, and
    /// one of these is built per annotated field.
    From(Box<Type>),
    /// Spellings the author listed, for a type no `Values` implementation can reach.
    ///
    /// Unverifiable by construction — nothing here reads the type's `Deserialize` — which is the
    /// standing `#[config(note = "…")]` already has, and is documented as such.
    Literal(Vec<String>),
}

impl ValueList {
    /// The `&'static [&'static str]` this reports, for a key or element whose type is `target`.
    pub(crate) fn tokens(&self, target: &Type, krate: &Path) -> TokenStream2 {
        match self {
            Self::Trait => quote! { <#target as #krate::schema::Values>::VARIANTS },
            Self::From(named) => quote! { <#named as #krate::schema::Values>::VARIANTS },
            Self::Literal(values) => quote! { &[#(#values),*] },
        }
    }

    /// The attribute suffix that asks for this, for an error that can quote back what to write
    /// instead of what was written.
    pub(crate) fn spelling(&self) -> &'static str {
        match self {
            Self::Trait => "",
            Self::From(_) => "_from",
            Self::Literal(_) => "(…)",
        }
    }
}

/// Compared by the *text* of the named type, because `syn`'s `PartialEq` lives behind the
/// `extra-traits` feature and this crate compiles `syn` without it — the comparison exists only to
/// tell one repeated attribute from two contradicting ones.
impl PartialEq for ValueList {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Trait, Self::Trait) => true,
            (Self::From(ours), Self::From(theirs)) => type_text(ours) == type_text(theirs),
            (Self::Literal(ours), Self::Literal(theirs)) => ours == theirs,
            _ => false,
        }
    }
}

impl Eq for ValueList {}

/// A `#[config(range(...))]` bound, in the form it was written.
///
/// The literal's own kind is kept rather than everything becoming an `f64`, because an `f64` holds
/// only the integers below 2^53 exactly and a bound that arrived rounded would be a *different*
/// number than the one the field takes.
#[derive(Clone, Copy)]
pub(crate) enum BoundLit {
    Integer(i64),
    Fractional(f64),
}

impl BoundLit {
    /// The expression reconstructing this as a `terrace_config::schema::Bound`.
    pub(crate) fn tokens(self, krate: &Path) -> TokenStream2 {
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
    pub(crate) fn approximate(self) -> f64 {
        match self {
            Self::Integer(value) => value as f64,
            Self::Fractional(value) => value,
        }
    }
}

/// Which end of a range a bound names.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edge {
    Lower,
    Upper,
}

impl Edge {
    /// The two spellings that set this end, for an error that can name both.
    pub(crate) fn spellings(self) -> &'static str {
        match self {
            Self::Lower => "`min` and `exclusive_min`",
            Self::Upper => "`max` and `exclusive_max`",
        }
    }
}

/// The bounds one field's `#[config(range(...))]` asks for.
#[derive(Default, Clone, Copy)]
pub(crate) struct RangeOpts {
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
    pub(crate) fn extend(&mut self, list: &MetaList) -> syn::Result<()> {
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
    pub(crate) fn as_written(self) -> (&'static str, &'static str) {
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
    pub(crate) fn tokens(self, krate: &Path) -> TokenStream2 {
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
pub(crate) fn bound_literal(expr: &Expr) -> syn::Result<BoundLit> {
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

impl FieldOpts {
    pub(crate) fn parse(field: &Field) -> syn::Result<Self> {
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
                Meta::NameValue(nv)
                    if nv.path.is_ident("with") || nv.path.is_ident("deserialize_with") =>
                {
                    opts.custom_deserialize = Some(path_ident(&nv.path));
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
                Meta::NameValue(nv) if nv.path.is_ident("reload") => {
                    opts.reload = Some(ReloadClass::parse(&nv.value)?);
                }
                Meta::NameValue(nv) if nv.path.is_ident("values_from") => {
                    let named = named_type(&nv.value, "values_from")?;
                    opts.set_values(ValueList::From(Box::new(named)), field)?;
                }
                Meta::NameValue(nv) if nv.path.is_ident("element_values_from") => {
                    let named = named_type(&nv.value, "element_values_from")?;
                    opts.set_element(ElementKind::Choice(ValueList::From(Box::new(named))), field)?;
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
                         `secret`, `skip`, `values`, `values(…)`, `values_from = \"…\"`, \
                         `element`, `element_values`, `element_values(…)`, \
                         `element_values_from = \"…\"`, `note = \"…\"`, `reload = \"…\"`, \
                         and `range(…)`.",
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
    pub(crate) fn check(&self, field: &Field) -> syn::Result<()> {
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

        // The trap `docs/SCHEMA.md` has described in prose since the feature shipped, checked:
        // a derived variant list is `serde`'s derived wire form, and each of these four says the
        // wire form is something else. A named type and a literal list are both the author saying
        // what the conversion accepts, so neither is refused.
        if let Some(written) = &self.custom_deserialize {
            let derived = match (&self.values, &self.element) {
                (Some(ValueList::Trait), _) => Some("values"),
                (_, Some(ElementKind::Choice(ValueList::Trait))) => Some("element_values"),
                _ => None,
            };
            if let Some(asked) = derived {
                return Err(syn::Error::new_spanned(
                    field,
                    format!(
                        "`#[config({asked})]` reports the variants `Describe` derives, which are \
                         `serde`'s derived wire form — and `#[serde({written} = \"…\")]` on this \
                         field says the wire form is something else. A case-insensitive `FromStr` \
                         is the conversion that turns up, and it accepts spellings no variant is \
                         named after, so the derived list would reject a file the loader takes. \
                         Name the type that does describe the accepted set with \
                         `#[config({asked}_from = \"…\")]`, list the spellings with \
                         `#[config({asked}(\"…\", \"…\"))]`, or `#[config(skip)]` the key."
                    ),
                ));
            }
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
    pub(crate) fn set_element(&mut self, kind: ElementKind, field: &Field) -> syn::Result<()> {
        match &self.element {
            Some(held) if held.attribute() != kind.attribute() => Err(syn::Error::new_spanned(
                field,
                "`#[config(element)]` and `#[config(element_values)]` describe the same element \
                 two ways. It is either a type with keys of its own, which `element` reports, or \
                 an enum of unit variants, which `element_values` reports.",
            )),
            Some(held) if *held != kind => Err(syn::Error::new_spanned(
                field,
                format!(
                    "`element_values{}` and `element_values{}` name the spellings one element \
                     accepts two different ways, and an element accepts one set. Bare, it is the \
                     element type's own `Values` implementation; `_from` is another type's; a \
                     list is the list.",
                    held.value_list().spelling(),
                    kind.value_list().spelling()
                ),
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
    pub(crate) fn set_values(&mut self, source: ValueList, field: &Field) -> syn::Result<()> {
        if let Some(held) = &self.values
            && *held != source
        {
            return Err(syn::Error::new_spanned(
                field,
                format!(
                    "`values{}` and `values{}` name the values this key accepts two different \
                     ways, and a key accepts one set. Bare, it is the field type's own `Values` \
                     implementation; `_from` is another type's; a list is the list.",
                    held.spelling(),
                    source.spelling()
                ),
            ));
        }
        self.values = Some(source);
        Ok(())
    }

    /// The key segment this field contributes, which must be the one serde will look for.
    pub(crate) fn name(&self, field: &Field, container: &Container) -> syn::Result<String> {
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
