# The contract specification

The configuration contract is a JSON document an image publishes saying what configuration it
takes. This directory is what that document *is*, independently of anything that produces one.

It exists because there is about to be more than one producer. While there was exactly one — this
crate, wrapping figment — the format could live as Rust types and prose about them, and every
consumer that could not read Rust reimplemented the vocabulary by reading the source. A second
producer in another language makes that untenable in both directions: the Java implementation has
nothing to conform *to*, and a consumer meeting a document has no way to know which implementation's
assumptions apply to it.

So the shared artefact between implementations is the **document format**, not an IDL and not a
code generator. Each language derives a contract from its own native types, using whatever its
ecosystem already annotates them with, and the thing they have in common is what comes out.

## Two version numbers

They move independently, and the directory is named after the first.

| Field | Now | Versions | Bumped when |
|---|---|---|---|
| `terrace_contract` | **1** | the envelope — which fields the document has at all | a change no consumer written against the old shape can read |
| `schema.schema_version` | **4** | the `schema` half — what a key may carry | a consumer walking `constraint`'s keywords itself has to widen its allowlist |

So a document in `spec/v1/` declares `"terrace_contract": 1` and, inside it, a `schema_version`
between 1 and 4. That is not a mismatch: the schema half reached 2 when `constraint` learned to
nest, 3 when it learned entry names (`propertyNames`) and 4 when it learned conditions between a
struct's fields, and none of that added anything to the envelope around it. A producer publishes
the lowest version whose vocabulary holds what the document carries
([`FORMAT.md`](v1/FORMAT.md#versioning)), so most documents still declare 2.

`spec/v1/` is the envelope's, because `terrace_contract` is what a consumer gates on before reading
anything else — a version it was not written against is a document it refuses, and that decision is
made from the outermost field. A second envelope version gets `spec/v2/`; a new `schema_version`
does not.

## Layout

```text
spec/
  README.md            this file
  v1/
    FORMAT.md          normative: what the document means
    CONFORMANCE.md     normative: what an implementation is held to, in three tiers
    PRODUCER.md        a guide: writing a producer in a language that has none
    contract.schema.json   machine-checkable: what a well-formed document is
    conformance/       reference documents, one directory per case
    producers/python/  a minimal reference producer, standard library only
```

Directory-versioned, because `$id` is a promise. `spec/v1/` describes `terrace_contract: 1` and will
not be edited to describe anything else; a second envelope version gets `spec/v2/`, and both stay
readable while consumers move.

## Where to start

| You are | Read |
|---|---|
| writing a consumer — a chart gate, a deployment check | [`v1/FORMAT.md`](v1/FORMAT.md), especially *Reading a container* and *Reading a variable* |
| writing a producer in another language | [`v1/PRODUCER.md`](v1/PRODUCER.md), with [`v1/producers/python/`](v1/producers/python/) beside it; then [`v1/CONFORMANCE.md`](v1/CONFORMANCE.md), the corpus and `FORMAT.md` |
| using the Rust implementation | [`../rust/docs/CONTRACT.md`](../rust/docs/CONTRACT.md) |
| validating a document you were handed | [`v1/contract.schema.json`](v1/contract.schema.json) |

## Using the meta-schema

`contract.schema.json` is self-contained: every `$ref` in it is a local pointer, so it needs no
resolver and no network. Vendor the file rather than fetching it — its `$id` is an identifier, not
a promise that something answers at that URL.

Two entry points. The root validates a whole contract. The `schema` half — what a producer's `json`
rendering emits on its own — is at `#/$defs/schema`, or by anchor at `contract.schema.json#schema`.

It is deliberately **open**: it checks that what is present is well-formed, not that nothing else is
present. The format evolves by addition within a version, and a consumer that ignores what it does
not recognise stays correct across every such addition. A producer emitting a field nobody asked for
is caught by the conformance corpus instead, where the diff shows exactly what appeared.

## Changing the spec

A change here is a change to every implementation, so the order matters:

1. Change `FORMAT.md` — the meaning is what is being changed, and the rest follows from it.
2. Change `contract.schema.json` to match, and check that the reference implementation still
   validates against it.
3. Re-bless the corpus, and read the diff. If the diff is empty, the change was to prose only. If it
   is not, that diff is the work every other implementation now has.
4. Say which tier of `CONFORMANCE.md` the change affects.

Additions are cheap and removals are not. Within an envelope version, a field may be added and a
field's meaning may be narrowed only in the direction of *accepting more*; anything else is `v2`.
