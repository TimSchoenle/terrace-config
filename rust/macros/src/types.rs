//! Reading a field's Rust type: which types describe themselves, which are containers of another,
//! and how a type is spelled in the table.

use quote::quote;
use syn::{Field, GenericArgument, PathArguments, Type};

use crate::field::ElementKind;

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
pub(crate) fn describes_something(field: &Field, ty: &Type) -> syn::Result<()> {
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
             `#[config(values_from = \"…\")]` if another type does, a `#[serde(remote)]` mirror \
             of a foreign enum being the case that turns up; `#[config(values(\"…\", \"…\"))]` \
             if none can, which asserts the accepted spellings the way \
             `#[config(note = \"…\")]` asserts prose; `#[config(nested)]` if it implements \
             `Describe` and its keys belong under this one; `#[config(element)]` or \
             `#[config(element_values)]` if the field is a container and it is the *element* \
             that is one of those; `#[config(range(…))]` if it is a number in an interval; and \
             `#[config(skip)]` if the field genuinely has nothing to publish."
        ),
    ))
}

/// Whether a type is a bare name `terrace_config` will read nothing from.
pub(crate) fn is_undescribed(ty: &Type) -> bool {
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
pub(crate) const KNOWN_LEAVES: &[&str] = &[
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

/// The error for `#[config(element)]` on a field this derive cannot find a container in.
///
/// Named rather than guessed at, because the guess would be silent and wrong: a schema saying
/// `routes` is an object because its element is a struct describes a file nobody can write. The
/// alias case is called out because it is the one that looks like a bug in the derive — the type
/// *is* a container, and a derive cannot see through a name to know it.
pub(crate) fn not_a_container(field: &Field, element: &ElementKind) -> syn::Error {
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
pub(crate) fn element_type(ty: &Type) -> Option<&Type> {
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
pub(crate) fn container_item(ty: &Type) -> Option<&Type> {
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
pub(crate) fn described_position(ty: &Type) -> &Type {
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
pub(crate) enum Step<'a> {
    /// A wrapper `serde` sees straight through: what it holds *is* the value.
    Through(&'a Type),
    /// A container's element position — a sequence's item, or a map's value.
    Into(&'a Type),
}

/// Past one wrapper, or into one container — or [`None`] for a type that is neither.
pub(crate) fn step(ty: &Type) -> Option<Step<'_>> {
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

/// A field's type as an operator would read it.
///
/// The token text, tidied: `Vec < String >` is what a `TokenStream` prints and `Vec<String>` is
/// what anyone documenting a configuration wants to see. Nothing is resolved — a type alias
/// prints as its alias and `SecretString` prints as itself — because a derive has no types, only
/// tokens. That is honest and it is what the field says; inventing a type language that claimed
/// otherwise would be worse than printing what is written.
pub(crate) fn type_text(ty: &Type) -> String {
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
pub(crate) fn is_option(ty: &Type) -> bool {
    unwrap_option(ty).is_some()
}

/// The `T` in `Option<T>`.
pub(crate) fn unwrap_option(ty: &Type) -> Option<&Type> {
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
