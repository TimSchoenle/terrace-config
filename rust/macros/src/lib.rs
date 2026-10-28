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
//!
//! `container` and `field` parse the attributes, `types` reads the Rust types, and `expand` emits
//! the impls; `attrs` holds the small `syn` readers the two parsers share.

mod attrs;
mod container;
mod expand;
mod field;
#[cfg(test)]
mod tests;
mod types;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

use crate::container::Container;
use crate::expand::{expand_enum, expand_struct};

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
/// | `#[config(values_from = "…")]` | Report another type's variants as the values the key accepts |
/// | `#[config(values("…", "…"))]` | Report a literal list as the values the key accepts |
/// | `#[config(range(…))]` | Bound the number the key accepts: `min`, `max`, `exclusive_min`, `exclusive_max` |
/// | `#[config(element)]` | Report the shape of one element of a container-typed key |
/// | `#[config(element_values)]` | Report the values one element of a container-typed key accepts |
/// | `#[config(element_values_from = "…")]` | Another type's variants, one level down |
/// | `#[config(element_values("…", "…"))]` | The same literal list, one level down |
/// | `#[config(skip)]` | Omit the key from the schema without affecting deserialisation |
/// | `#[config(reload = "…")]` | On a struct or a field: whether a rebuild applies a change — `live` or `restart` |
/// | `#[config(crate = "…")]` | Name the `terrace_config` crate, if it was renamed |
///
/// `nested` is opt-in because no macro can tell a `PathBuf` from a nested config struct by
/// looking at the type: both are one identifier and a module path. Guessing would mean either
/// bare identifiers silently becoming leaves, or a bound on types that cannot satisfy it.
///
/// # Reloading
///
/// `#[config(reload = "live")]` says a rebuild applies a change to every key under it — the value
/// is consumed inside the runtime `reload::run` reconstructs. `#[config(reload = "restart")]` says
/// only a process start does: the value is read before the supervisor runs, or changing it under
/// live traffic is unsafe. On a struct it covers every field, nested types included; on a field it
/// covers that key, or everything under it for `nested`.
///
/// ```ignore
/// #[derive(Deserialize, Describe)]
/// #[config(reload = "live")]
/// struct Config {
///     /// Read by the `tracing` subscriber, which is installed before `reload::run`.
///     #[config(reload = "restart")]
///     log_level: String,
///     /// Applied by the next rebuild.
///     #[serde(default)]
///     ttl_secs: u64,
/// }
/// ```
///
/// **An explicit `restart` anywhere on a key's path wins**, over a `live` above it and below it
/// alike. A key nothing marks is undeclared, and the contract publishes it as `restart` once the
/// loader declares what the binary supports — so `live` is always something an author wrote.
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
/// `#[config(values)]` reads the field type's own `Values` implementation, and the orphan rule
/// puts a foreign enum out of reach: an application can implement neither `Values` nor `Describe`
/// for `other::Compression`, and this crate will not depend on `other` to do it here. Two
/// attributes reach past the trait, and they are not equals.
///
/// **`values_from` names a type that does implement it.** A `#[serde(remote = "…")]` mirror is the
/// case it exists for: serde matches the mirror's variants exactly, `Describe` on the mirror
/// reports those same variants under the same `rename_all`, and the two cannot drift because they
/// are one declaration. Nothing is spelled twice, and the compiler still checks it:
///
/// ```ignore
/// #[derive(Deserialize, Describe)]
/// #[serde(remote = "other::Compression", rename_all = "lowercase")]
/// enum CompressionDef { Gzip, Zstd, None }
///
/// /// How payloads are compressed.
/// #[serde(with = "CompressionDef", default)]
/// #[config(values_from = "CompressionDef")]
/// compression: other::Compression,
/// ```
///
/// **`values("…", "…")` lists the spellings**, for a type no mirror can be written for — serde's
/// `remote` needs a shape it can mirror, and a newtype over a private enum has none:
///
/// ```ignore
/// /// How payloads are compressed.
/// #[serde(deserialize_with = "compression", default)]
/// #[config(values("gzip", "zstd", "none"))]
/// compression: other::Compression,
/// ```
///
/// Both have element forms — `element_values_from = "…"` and `element_values("…", "…")` — for a
/// container of one. All four satisfy the diagnostic above, and bare `#[config(values)]` is
/// unchanged.
///
/// **A list is an assertion this crate cannot check**, the same standing as
/// `#[config(note = "…")]`: nothing here reads the type's `Deserialize`, so a list that disagrees
/// with it publishes a schema rejecting a file the loader takes. Prefer `values_from` wherever a
/// mirror can be written — variants read off a type cannot drift from it. An empty list and a
/// repeated spelling are rejected.
///
/// # The accepted set is the deserialiser's answer, not the type's
///
/// Two shapes are refused outright, because in both the derived variants provably are not the wire
/// form: an enum carrying `#[serde(try_from = "…")]` or `#[serde(from = "…")]` cannot derive
/// `Values` at all, and a field carrying `#[serde(with = "…")]` or
/// `#[serde(deserialize_with = "…")]` cannot take a bare `values` or `element_values`. Both read
/// something else and convert, and what the conversion accepts is a path this derive cannot
/// follow.
///
/// `tracing::Level` is the worked example of why, and the one to *not* copy. It has no
/// `Deserialize` of its own, so a field holding one goes through its `FromStr` — which matches
/// case-insensitively **and** accepts `"1"` through `"5"`. A field annotated
/// `values("trace", "debug", "info", "warn", "error")` therefore publishes a schema refusing
/// `INFO` and `3`, both of which load, which is the one thing this crate will not do. Read the
/// accepted set off the conversion rather than off the variant names; where it is not a fixed set
/// of spellings at all, `#[config(skip)]` the key or implement `Describe` by hand.
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
