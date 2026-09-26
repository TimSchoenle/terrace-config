# The configuration contract, envelope version 1

> **`terrace_contract: 1`, containing `schema_version: 2`.** Two numbers, moving independently —
> this directory is named after the envelope's. See [Versioning](#versioning).

**Status: normative.** This file defines the document. Everything else describing it — the Rust
crate's [rustdoc], [docs/CONTRACT.md], a future Java implementation's javadoc — is a guide to
producing or consuming it and defers to this file where the two differ.

A contract is one JSON document, attached to one image digest, saying what configuration that image
takes. It exists because the renderings that describe a configuration to a person assume the reader
has the source. A Helm chart's CI job does not: it holds an image digest and a `config.toml` it
rendered itself, and no way to know whether the two agree. So a chart renders `isr.ttl_secs`, the
service renames the key, the deserialiser ignores what it does not recognise, and the pod starts
healthy on a compiled default.

Machine-checked against [`contract.schema.json`](contract.schema.json). Where this prose and that
schema disagree about whether a document is well-formed, the schema is wrong and should be fixed —
but a consumer may rely on the schema, so the fix is a spec change, not a reading.

[rustdoc]: https://docs.rs/terrace-config
[docs/CONTRACT.md]: ../../rust/docs/CONTRACT.md

## Conventions

**MUST**, **MUST NOT**, **SHOULD** and **MAY** are used in the RFC 2119 sense. "Producer" is
whatever writes the document — a build-time generator reading a service's types. "Consumer" is
whatever reads it — a chart's CI job, a deployment gate, an editor.

Every field name in this document is `snake_case`, the envelope's own included. One document in
two conventions — `text_constraint` on a key and `textConstraint` on the variable beside it — is a
field name a consumer gets right from memory under one convention and guesses at under two.

## The envelope

```json
{
  "terrace_contract": 1,
  "producer": { "name": "…", "version": "…", "loader": "…" },
  "app":      { "name": "…", "version": "…" },
  "schema":   { "schema_version": 2, "dialect": { … }, "loader": [ … ], "reload": { … }, "keys": [ … ] },
  "json_schema": { "$schema": "http://json-schema.org/draft-07/schema#", … },
  "external": { "env": [ … ], "ignore": [ … ], "unknown": "reject" }
}
```

All six fields MUST be present. Inside `schema`, `reload` is the one optional member; see
[Reloading](#reloading).

Both schema halves are there because neither is enough on its own. `json_schema` is the only one a
stock JSON Schema validator can act on, and it carries no environment spellings at all — so it
cannot tell a chart that the `PORTFOLIO_ISR__CACHE_DIR` it sets is no longer read, or that the file
it mounts as `github__token` is now named something else. `schema` carries every spelling and can
be handed to no validator. Published as two artefacts they would be two hashes and two chances to
be half-stale.

### `producer`

Which implementation wrote the document, and which loader it describes. Not a fact about the
configuration; a fact about how to read the rest of the file.

| Field | Meaning |
|---|---|
| `name` | The implementation, e.g. `terrace-config`. A stable identifier, not a display name. |
| `version` | That implementation's own version. Not `app.version`. |
| `loader` | The library whose environment reads every `text_constraint` here was measured against, e.g. `figment`. |

`loader` is the load-bearing one. The text patterns in this document are **not** derived from a
grammar — they are measured against the library the image actually links, and a different library
has different answers about `+5`, `007`, `TRUE`, and how a list is spelled in one variable. A
consumer MUST NOT apply the reads in [Reading a variable](#reading-a-variable) to a document whose
`producer.loader` it does not recognise; it MUST report the check as skipped rather than passed.

It names the loader rather than restating its rules. A vocabulary rich enough to describe every way
a binder might read a string is a language two implementations can disagree in, and this format
carries as little of that as it can — the single trailing `*` in `external.ignore` is the whole of
its pattern syntax, for the same reason.

### `app`

Which build this describes: `name` (required), and optionally `version`, `revision`, `created`,
`source`. Collected rather than scattered so that a consumer diffing two contracts to see whether
the *configuration* changed can diff everything except this.

Nothing in the document names the image, deliberately. A digest is what building the image
produces, so a field carrying it could only be written after the push — changing bytes that were
already hashed and already committed. The tie is the attachment; see
[Publication](#publication).

## Versioning

Two version numbers, moving independently.

**`terrace_contract`** versions the envelope. A consumer MUST refuse a version it was not written
against rather than read it optimistically.

**`schema.schema_version`** versions the schema half. Version 2 allowed `constraint` to nest.
Version 3 added `propertyNames` (see [Refinements](#refinements)). A consumer gates its keyword
allowlist on this: refuse a version you were not written against, widen, then accept it.

A producer publishes the **lowest** version whose vocabulary holds every keyword the document
carries, not the newest it can write. Each version's vocabulary contains the last, so a document
using nothing new reads exactly as it did, and a consumer is told to widen only by a document it
would otherwise under-check.

The allowlist is of keywords, not of positions. A constraint is one JSON Schema, and a keyword a
version permits anywhere in it is permitted everywhere in it — so `required` at the top of a
map-typed key's constraint (see [Refinements](#refinements)) is an addition within version 2: the
keyword has been in version 2's vocabulary from the start, inside every element schema with a
mandatory field.

Within a version the format evolves **by addition only**. A consumer that ignores fields it does
not recognise stays correct across every such addition, and `contract.schema.json` is deliberately
open at every object level for that reason: it checks that what is present is well-formed, not that
nothing else is. Producer typos are caught by the [conformance corpus](CONFORMANCE.md), not by the
meta-schema.

Several fields are closed enumerations with a degradation target — `text_form: unknown`,
`unreachable: other`, `role: other`, `unknown: reject`, `reload.mode: none`, a key's
`reload: restart`. Degrading is still degrading: **a consumer
that skips a check because it did not recognise a value MUST say that it did.** A silently skipped
check is indistinguishable from a passing one, which is the failure this whole document exists to
prevent one level up.

## The schema half

### `dialect`

How a key path becomes a name in each layer.

| Field | Meaning |
|---|---|
| `prefix` | The environment namespace this contract governs, e.g. `PORTFOLIO_`. |
| `nesting_separator` | What separates path segments, e.g. `__`. |
| `indirection_suffix` | What marks a variable as naming a file rather than holding a value, e.g. `_FILE`. |

`prefix` MUST NOT be empty. A prefixless loader cannot tell its own namespace from the machine's,
and every gate in [Reading a container](#reading-a-container) rests on that distinction — step 4
would fire for every variable on the container, because every name begins with the empty string.

A consumer MUST NOT derive a key's spellings from the dialect itself. The dialect is published so
that step 4 has a namespace to test against; the spellings are published per key, because deriving
them involves case folding, separator collisions and round-trip checks that prose gets differently
wrong in each language that reimplements it.

### `loader[]`

The variables the loader reads to decide what the layers *are*, rather than to fill a key.

| Field | Meaning |
|---|---|
| `env` | The variable's name. |
| `role` | `config`, `secrets_dir`, `reserved`, or `other`. |
| `docs` | Prose. |
| `default` | What the loader assumes when it is unset, or `null`. |

A `reserved` variable is read directly from the environment before the layers exist, so no file may
supply it and it appears in no rendered document.

### `reload`

Whether the image applies a configuration change without restarting, and through which layers.
Optional; see [Reloading](#reloading) for what its absence means.

| Field | Meaning |
|---|---|
| `mode` | `none` or `rebuild`. |
| `layers` | The file layers the image watches: `document`, `secrets_dir`, `env_file`. |

### `keys[]`

One entry per configuration key. `path` is unique across the array.

| Field | Meaning |
|---|---|
| `path` | The document path, e.g. `csp.cloudflare.turnstile`. |
| `env` | The variable supplying it directly, or `null`. |
| `env_file` | The variable naming a *file* holding it, or `null`. |
| `secrets_file` | The file name inside the secrets directory that supplies it, or `null`. |
| `docs` | Markdown prose; the first paragraph is the summary. Empty when there was none. |
| `ty` | The producer's own name for the field's type, optionality stripped. |
| `values` | The fixed set of values the key accepts. Empty when it is not a choice. |
| `constraint` | What the value must be *in the document*. Absent when nothing certain can be said. |
| `text_constraint` | What the characters of a variable must be *before parsing*. Absent likewise. |
| `text_form` | How to read those characters. Always present. |
| `aliases` | Other document spellings that also load. |
| `env_aliases`, `env_file_aliases`, `secrets_file_aliases` | Those aliases' spellings in each layer. |
| `unreachable` | Why no `env` is published. Absent when there is nothing to explain. |
| `default` | The observed default, rendered for display, or `null`. |
| `default_value` | The same default as a value rather than as text, or `null`. |
| `note` | Prose qualifying the default, e.g. `permanent` for a zero meaning no expiry. |
| `required` | Whether some layer must supply the key. |
| `secret` | Whether the value is a credential. |
| `reserved` | Whether the loader reads it before the layers exist. |
| `reload` | `live` or `restart`: whether a rebuild applies a change to it. Absent when undeclared. |

**`ty` is not a portable type name.** It is token text in the producer's language: a type alias
appears as its alias, a domain newtype as itself. It is published so a person reading a table can
see what the field is; a consumer checking a value MUST read `constraint`, never `ty`. Without that
rule every consumer in every language reimplements a vocabulary of one language's type names, with
`PathBuf` as the trap — it is a string and nothing in the name says so.

**`required` is not JSON Schema's `required`.** JSON Schema's says *this document must carry the
property*. This one says *some layer must supply the key*, and the loader takes the environment or
a mounted file just as readily. A required **secret** is supplied by a mount — the only way to
supply a secret — so the rendered document does not carry it at all. A consumer checks `required`
per key across every layer it can see, and can then say "no layer supplies this" rather than "add
it to the file", which for a credential is the wrong advice.

**A `secret` MUST NOT carry a default**, in either field, anywhere in the document. This is the
point the document crosses into a public registry, and a producer MUST refuse to build one rather
than emit it.

**Aliases are not decoration.** A key answers to every one of its spellings, so a chart still using
a name kept alive by an alias is a *correct* deployment. A consumer that checks only the canonical
spelling sends it to step 4 of [Reading a container](#reading-a-container) and rejects it, turning
the shim that makes a rename safe into the thing that fails the gate.

**`env: null` MUST be accompanied by `unreachable`.** The two reasons differ in whether the
environment can reach the key at all:

- `unnameable` — no variable names it. A `rename_all = "camelCase"` path never comes back through
  the case fold, and neither does one carrying the nesting separator. The document is the only
  layer left.
- `indirection` — a name this schema gives to another key also fills this one. A producer MUST
  refuse to build a contract containing this; it appears only in a schema rendered for
  documentation.

A consumer meeting a bare `null` and treating it as "skip this key" is right for the first and
wrong for the second, which is why the reason is published rather than inferred.

**`default_value: null` is ambiguous on its own** — it is what both "no default" and "the default
is null" render to. Read `default` and `required` alongside it.

**A `default_value` MUST satisfy the key's own `constraint`.** A key publishing a default says
leaving it out is fine and names what it falls back to; a constraint rejecting that value says the
fallback is invalid. The two cannot both be true, and a consumer generating fixtures or values from
the default builds a deployment that fails at boot. Where the two would disagree because a
[refinement](#refinements) tightened the constraint, the default is not a default: the key is
`required: true` and carries none. A producer MUST refuse to build any other disagreement.

## The two spaces

A configuration value exists in two forms and a validator meets both. In a TOML file `ttl_secs = 0`
is an integer; in the environment `PORTFOLIO_ISR__TTL_SECS=0` is the two characters `"0"`, and
`"0"` fails `{"type": "integer"}` under every conforming JSON Schema validator. So each key carries
two constraints, flat, beside the spellings:

```json
{ "path": "isr.ttl_secs", "env": "PORTFOLIO_ISR__TTL_SECS", "ty": "u64",
  "constraint":      { "type": "integer", "minimum": 0 },
  "text_constraint": { "type": "string", "pattern": "^\\s*\\+?[0-9]+\\s*$" },
  "text_form":       "integer" }
```

`constraint` describes the parsed value; `text_constraint` describes the characters a variable holds
before anything parses them. They are complementary, not alternatives.

`json_schema` carries `constraint` again, nested, at the key's position in the document. The flat
copies are for the consumer that has a variable name and a string rather than a document.

**A pattern here is a superset of what the loader accepts, never a subset.** A schema that rejects
text the loader takes stops a deployment that was correct, which is worse than one that accepts text
the loader will reject — the second fails at boot with a real error message, the first fails at a
gate with a false one. Every keyword a producer emits MUST be certainly true of the loader it
describes, and a producer that cannot say anything certain MUST omit the field rather than guess.

`constraint` MAY nest, from `schema_version: 2`: a container-typed key whose element type describes
itself carries the element's shape under `items` for a sequence and `additionalProperties` for a
map, composed as deeply as the containers are stacked. It is still one key with one row in `keys` —
an array index is not a key segment and no variable names one.

An element schema is **open unless the element type closed it**. A deserialiser that accepts an
undeclared field unless told otherwise means open is the only safe default; a type that did say
otherwise carries `additionalProperties: false` at that level and no other.

### Refinements

A producer MAY publish a `constraint` **tighter than the type states**, and MUST NOT publish one
looser. Some constraints are real without being in any type — a host that refuses to start unless a
map holds an `imprint` entry — and a contract that cannot carry them publishes `{}` as a valid value
for a map the image rejects. Tightening is safe for the reason every rule in this section is: the
value the loader would accept and the service would then refuse is not a correct deployment, so a
gate refusing it is right. Loosening is never safe, because the type's own constraint is what the
loader enforces.

The first tightening is **required entries of a map.** A key whose `constraint` is an object
with no `properties` — a map, its element schema if any under `additionalProperties` — MAY carry
`required`, a sorted list of entry names the map must contain. It means exactly what JSON Schema's
`required` means at that position: a document supplying the map must supply those entries. The map
stays open; other entries are accepted as the type accepts them. It is carried inside `constraint`
and therefore inside `json_schema` at the key's position, and nowhere else: a consumer showing a
map's required entries reads them from `constraint.required`.

That `required` and the key's own `required` field answer different questions, as the field and JSON
Schema's keyword always have: the field says whether some layer must supply the key, the keyword
what the map must hold once supplied. They meet only through the default: a refined map whose
observed default lacks a required entry is published `required: true` with no default, under the
rule on `default_value` above. A refined map with no default at all keeps whatever `required` its
type gave it — absence is the type's statement, and a refinement of the value does not reach it.

An entry name MUST be spellable wherever the key is: nested one level under the key's `env`, and
under its `secrets_file`, by the same derivation the key's own spellings use. A producer MUST refuse
a refinement naming an entry some layer reaching the key could not spell — `Imprint` under a
lower-casing environment layer, a name carrying the nesting separator or a `.`, one whose spelling
ends in `indirection_suffix`.

A second tightening is defined: **entry names of a map**, from `schema_version: 3`. A map-typed key's
`constraint` MAY carry `propertyNames: {"pattern": P}`: every entry name the map holds must match
`P`, as JSON Schema's `propertyNames` means at that position. `P` MUST lie inside the
[portable subset](#portable-patterns). A key carries at most one such pattern.

The two tightenings MUST agree: every name in `required` matches the pattern, since a required entry
the pattern rejects makes the key unsatisfiable. A producer MUST refuse the second of two refinements
that would publish that, whichever arrived first. A default naming an entry the pattern rejects is,
like one lacking a required entry, not a default: the key is published `required: true` with none.

The pattern describes names in document space. The environment layer folds a variable's name to lower
case before reading it, so a pattern admitting an upper-case name describes an entry only a file can
supply. That is weaker than the loader, not wrong, and a producer publishes the pattern as the
runtime check states it.

A third tightening is defined: **a string's pattern.** A string-typed position MAY carry `pattern`,
from the [portable subset](#portable-patterns), which means what JSON Schema's `pattern` means there.
`pattern` has been in version 2's vocabulary from the start, so it does not raise the version. A
position carries at most one, and a choice (`enum`) none of whose values the pattern matches is
unsatisfiable and MUST be refused.

**Every tightening may sit inside a key**, not only at its top: at the element schema of a map
(`additionalProperties`) or a sequence (`items`), and at a declared field of an element struct
(`properties`), as deeply as the type described. A tightening inside a key applies to every entry or
item that position describes, as JSON Schema applies anything at that position. An entry named by
`required` inside a key MUST still be spellable wherever the key is, the element's own segment
standing for any entry name.

A consumer showing tightenings names each by its **position relative to the key**: the segments
from the key's constraint down to it, `.`-joined, with `*` for an element and a field's name for a
field — `*.body` for the `body` field of every entry, `*.body.*` for every text in it. The key's own
tightenings have the empty position. A consumer reads them by walking `constraint` in this order:
at each position, `required` (on a map only — a struct's `required` names fields), then a
`propertyNames` holding only `pattern`, then `pattern`; then the element, `additionalProperties`
before `items`; then each field in the order `properties` lists them.

A tightening's meaning never depends on how a consumer displays it; the renderings in the
[conformance corpus](CONFORMANCE.md) pin one way to say each, for the implementations that share
those goldens.

### Portable patterns

A pattern a refinement publishes is read by whatever validates the contract — an ECMA-262 engine
behind a JSON Schema validator, Rust's `regex`, Java's `java.util.regex`, Python's `re` — and those
disagree about a great deal. A producer MUST refuse a refinement whose pattern lies outside this
subset, in which each of them reads every construct alike:

- **Literals.** Any character from U+0000 to U+FFFF except the syntax characters
  `^ $ \ . * + ? ( ) [ ] { } |`, which are escaped to mean themselves.
- **Escapes.** A syntax character; `\t`, `\n`, `\r`, `\f`; and `\uXXXX`, exactly four hex digits
  naming a character that is not a surrogate. Inside a class, `\-` as well.
- **Classes.** `[…]` and `[^…]`, holding characters and ascending ranges of them. A literal `-`
  stands first or last, or is escaped; `[` is escaped; `--`, `&&`, `~~` and `||` do not appear,
  since Rust and Java read them as set operations. An empty class is not written.
- **Groups.** `(…)` and `(?:…)`, with `|` between alternatives.
- **Repetition.** `?`, `*`, `+`, `{n}`, `{n,}` and `{n,m}`, with `n ≤ m ≤ 1000`. Greedy only; a
  repetition is not itself repeated.
- **Anchors.** `^` and `$`, meaning the start and the end of the whole text.

Matching is JSON Schema's: unanchored, over characters. Nothing else is in the subset — in
particular `.`, whose line terminators differ by engine; `\d`, `\w`, `\s`, `\b` and their
negations, which are ASCII in one engine and Unicode in the next (ECMA-262's `\s` holds U+FEFF, which
Unicode's `White_Space` does not); back-references, lookaround, lazy repetition, flags, and any
character beyond U+FFFF. Each has a spelling inside the subset: a class naming exactly the characters
meant.

A consumer applies a pattern with the meaning above. Two engines need telling: Java's and Python's
`$` also match before a final line terminator, so a consumer in either reads `$` as `\z` or `\Z`
respectively. One difference survives, observable only by an ECMA-262 engine run without the `u` flag
and only through a negated class: such an engine reads a character beyond U+FFFF in the *value* as
two, and a negated class matches each half.

### `text_form`

Always present, so that a null `text_constraint` has one meaning rather than two: `text` says any
text is fine and there is nothing to parse, `unknown` says nothing could be determined — a gap
rather than an answer. Those used to be indistinguishable, and a list-typed key is what made the
difference cost a deployment.

| Value | The key holds |
|---|---|
| `text` | Any string. |
| `integer` | A whole number. |
| `boolean` | A truth value. |
| `choice` | One of `values`. |
| `structured` | A composite — a sequence, a map — which a single variable must spell as a literal. |
| `unknown` | Nothing could be determined. |

### Reading a variable

Checking a variable is **two steps**, and both are needed.

1. **Form.** The text MUST satisfy `text_constraint`, when there is one. `"http"` is not an integer
   in any spelling, and this is the check that says so.
2. **Range.** *Read* the text according to `text_form`, then check the result against `constraint`.
   This is where `minimum`, `maximum`, `minLength` and a document-space `enum` live, and it is the
   only step that can reach them: a pattern matches characters, so `99999` is a well-formed integer
   and only a bound catches it not fitting a `u16`.

Skipping the second leaves every bound in the document decorative — a deployment that passes every
gate and fails at boot. Applying `constraint` to the raw text instead rejects `"0"` for an integer
key, which is a correct deployment refused.

**How to perform the read in step 2 belongs to `producer.loader`, not to this format.** The reads
below are normative for `producer.loader == "figment"` and for nothing else. A consumer meeting a
loader it does not know MUST skip step 2 and say so.

<a id="figment-reads"></a>
For `figment`:

| Layer | Read |
|---|---|
| environment | trim all surrounding whitespace, then the form's read |
| secrets file, `_FILE` target | strip trailing line terminators, and nothing else |
| the document | nothing; it is already a parsed value |

| `text_form` | Read |
|---|---|
| `integer` | trim, drop a leading `+`, parse as an integer |
| `boolean` | trim, compare to `true` |
| `choice` | trim |
| `structured` | trim, parse as a TOML literal |
| `text`, `unknown` | trim |

Every read begins by trimming, because the environment layer trims before it parses anything — for
a plain string and a single character as much as for an integer. A read that skipped it would refuse
`" x "` against a `minLength` of 1, on a value that loads.

The file layers strip trailing `\r` and `\n` and no other whitespace: every editor and every YAML
block scalar adds a line ending nobody meant as part of the value, whereas a trailing space can be a
real character of a real password. So `"x\n"` supplies a single-character key and `"x "` does not.

This asymmetry is why a consumer holding a mounted secret checks its contents against `constraint`
and **not** against `text_constraint` — that one permits the surrounding whitespace an environment
spelling may carry and a file keeps.

<a id="terrace-java-reads"></a>
For `terrace-java`:

| Layer | Read |
|---|---|
| environment | the form's read; no separate trim step (Jackson's own scalar coercions already trim internally) |
| secrets file, `_FILE` target | strip trailing line terminators, and nothing else |
| the document (a TOML layer) | nothing; `tomlj` has already produced a parsed value |

| `text_form` | Read |
|---|---|
| `integer` | drop surrounding whitespace, an optional leading `+` or `-`, then parse as a base-10 integer; a **blank string coerces to `0`** rather than being refused (Jackson's own primitive-from-blank-string rule) — a producer publishing this loader accepts that a merged-but-empty variable reads as zero, not as absent |
| `boolean` | drop surrounding whitespace, then accept exactly `true`/`True`/`TRUE` or `false`/`False`/`FALSE`; no other spelling, `1`/`0` included |
| `choice` | drop surrounding whitespace, then compare verbatim |
| `text`, `unknown` | no read beyond the trim already reflected in `text_constraint` |

**No `structured` read exists for this loader.** `terrace-config-loader`'s environment and file
layers hand every value to the merged configuration map as a bare string; nothing in that path
parses a JSON or TOML literal out of it the way `figment` does. A `structured` key is therefore
reachable **only** through the TOML document layer (where `tomlj` has already produced the list or
table) — never through its own environment variable, secrets file, or `_FILE` target, whatever
`text_constraint` a document happens to publish for it. This is a real, current limitation of
`terrace-java`, not a spec choice: a future version of this loader closing it would only ever *add*
a read this table does not yet have, never change one that is here.

Integer coercion was measured against Jackson's default `ObjectMapper`, the one
`terrace-config-loader` binds every layer through: `"007"` reads as `7` (no octal interpretation),
`"1_000"` and `"0x5"` are both refused, and `"5.0"` is refused rather than truncated. Boolean
coercion accepts only the three case spellings Jackson's own error message names — not
case-insensitive matching against `true`/`false` in general, and not `"1"`/`"0"`.

### What the file layers cannot supply

A key-named file in the secrets directory and a `_FILE` target both deliver their contents as
strings with no parse. A key whose **`constraint` is not a string type** therefore **cannot be
supplied by either, whatever the file contains** — not "must match a pattern", cannot be supplied at
all. A chart mounting `isr__ttl_secs` as a secret file has made a mistake no file contents can fix,
and a validator can say so from `constraint` alone.

Keyed on `constraint`, not on `text_form`: that field answers what parse the *environment* needs,
and the two differ for every type whose deserialisation parses a string. An address-typed key is
`text_form: unknown` — no pattern here describes an address — and it is a string in the document and
mounts from a secrets file perfectly well.

## Reloading

A process picks up a changed file only if it watches the file and rebuilds from it, and applies a
changed value only if the value is consumed inside what it rebuilds. Neither is visible from
outside the binary. A deployment that restarts its processes on every change throws the reload
away, and one that restarts them on none leaves every value consumed at start changed on disk and
never applied. So the document states both, and a consumer derives the third fact — how its own
deployment delivers each key.

### What the document states

**`schema.reload` is a fact about the binary**, not about `producer.loader`: a service can link a
loader with a supervisor and never run it. `mode: rebuild` says a debounced change to any of
`layers` re-reads every layer and rebuilds the runtime. `mode: none` says nothing is applied after
start. **Absent means undeclared** — a document written before this field existed — and a consumer
treats it as `none` and reports it as undeclared rather than as none.

`layers` never contains the environment. A process's environment is fixed for its lifetime, so no
producer could truthfully publish it.

**`keys[].reload` is a fact about each value.**

- `live`: the value is consumed inside the rebuilt runtime, so a rebuild applies it.
- `restart`: the value is applied only at process start. Either it is consumed before the supervisor
  runs — a process-global installation, a `reserved` key — or changing it under live traffic is
  unsafe and the author wants a rollout's readiness gating and rollback instead. The format does
  not distinguish the two, because no consumer acts differently on them.

Absent means undeclared and is read as `restart`. A producer that publishes `schema.reload` SHOULD
publish `reload` on every key, so that `restart` is a statement rather than a default.

**A rebuild MUST keep every `restart` key at its boot value.** A runtime that rebuilds from the
whole re-read configuration applies a `restart` key the moment any `live` key beside it changes,
which makes the classification a claim nothing enforces. A producer publishes `mode: rebuild` only
for a runtime that pins.

Both enumerations are closed, with a degradation target: an unknown `mode` reads as `none`, an
unknown `reload` as `restart`, and under [Versioning](#versioning) a consumer says it degraded.
Every degradation is toward a needless restart, never toward a change silently not applied.

### Deciding whether a change needs a restart

Normative, and ordered for the reason [Reading a container](#reading-a-container) is. For one key,
delivered through one channel to a container running one image, first match winning:

1. The channel is the environment — a plain variable, an alias, or an indirection variable itself
   rather than the file it names. **Not applicable**: changing a variable changes the pod
   specification, and the orchestrator replaces the process without being asked.
2. The container runs once per pod — an init container. **Restart.**
3. The image's contract has no `schema.reload`. **Restart**, reported as undeclared.
4. `schema.reload.mode` is not `rebuild`. **Restart.**
5. The key's `reload` is absent, unknown, or `restart`. **Restart.**
6. The channel's layer is not in `schema.reload.layers`. **Restart.**
7. The channel is a mount the orchestrator never updates in place — a Kubernetes `subPath` mount,
   or an object marked immutable. **The deployment is defective**: the key is neither reloaded nor
   restarted when it changes. A consumer MUST report it rather than classify it.
8. Otherwise **live**.

A workload needs a restart when a change touches any key that is `restart` for any of its
containers. A key one image reads `live` and another reads `restart`, in a document both read, is
`restart` for every workload running the second.

Content a consumer cannot attribute to a key — a verbatim fragment appended to a rendered
document — is `restart` content.

## The `json_schema` half

The same keys as a JSON Schema, for validating the document a chart renders. Draft-07 by default,
because that is the dialect Helm validates `values.schema.json` against, and `additionalProperties:
false` by default, because an unknown key is the defect the document exists to catch and an open
schema catches none of them.

It carries **no `required` list** by default, for the reason given under `required` above.

Every alias is a property of its own: a closed schema listing only the canonical spelling would
underline a line that loads. A `reserved` key is left out entirely — the loader reads it from the
environment and a file may not supply it.

## The `external` half

A service reads variables that are not its configuration: a toolchain's `PORT`, a base image's
`SSL_CERT_FILE`, the platform's `KUBERNETES_*`. None carry the loader's prefix, so no producer can
discover them, and a validator that flagged everything it could not account for would flag all of
them.

A **positive declaration**, deliberately, rather than a suppression list:

| | What it says | What a validator does |
|---|---|---|
| `env[]` | this image reads it, and here is its type | checks it exactly like a configuration key |
| `ignore[]` | nobody here owns it | skips it |
| `unknown` | what to do with everything else | `reject` by default |

An entry in `env[]` has the same two-space fields a key has, minus the spellings. A declared `PORT`
with `ty: "u16"` means a chart passing `PORT: "http"` fails the same gate a bad configuration key
fails. An ignored `PORT` is a variable the chart may misspell freely. Reach for `ignore` only where
there is genuinely no owner.

**Only a trailing `*` is a wildcard.** Every consumer implements this matching itself, and a pattern
language is a place for two implementations to disagree about what is exempt from a check. A pattern
MUST NOT be empty and MUST NOT contain `*` anywhere but at its end.

## What a producer MUST refuse

A producer MUST fail rather than emit a document containing any of these. Each is a way a contract
could quietly stop being one.

1. An external variable **carrying `dialect.prefix`** — everything in that namespace is a
   configuration key, and declaring one external would leave it governed and exempt at once.
2. An ignore pattern **reaching into that namespace**, which is the same exemption through the
   other door and worse, because a pattern exempts everything it happens to cover. That includes a
   pattern that does not carry the prefix but subsumes it: `PORT*` against `PORTFOLIO_` reads as a
   pattern about the external `PORT` and disables the whole gate. An exact `PORT` is fine.
3. An external variable **colliding with a spelling the loader reads**, reached through a reserved
   name.
4. An ignore pattern **covering a spelling the loader reads**. The prefix is not the whole
   namespace: the config and secrets-directory variables take arbitrary names, so `CREDENTIALS_*`
   against a `CREDENTIALS_DIR` would exempt the variable deciding where every credential is read
   from.
5. An external variable **declared twice** — refusing to build beats picking one of two
   descriptions.
6. A **secret carrying a default**, in either order the two were declared and anywhere in the
   document.
7. An **empty `dialect.prefix`**.
8. A key whose environment spelling is **another key's indirection variable** — `unreachable:
   indirection`. With `token` and `token_file` both present, setting `<PREFIX>TOKEN_FILE` fills
   `token` from the file it names *and* fills `token_file` with the path: one variable, two keys,
   and a validator classifying it stops at the first. Publishing a contract that cannot describe an
   effect is worse than publishing none, because every gate downstream would pass.
9. A key whose **`default_value` fails its own `constraint`** — a default the document itself calls
   invalid. The refusal is for a failure the producer can prove: a keyword its evaluator does not
   implement leaves the answer undecided, and an undecided default is published rather than refused
   on a guess. A refinement MUST NOT be how a producer reaches this; see
   [Refinements](#refinements).
10. A key published **`reload: live` in an image that does not rebuild** — `schema.reload` absent or
    its `mode` not `rebuild`. The key claims a rebuild applies it, and the same document says there
    is no rebuild.
11. A **`reserved` key published `reload: live`**. It is read before the layers exist, from an
    environment that cannot change.
12. **`mode: rebuild` with an empty `layers`** — `none`, spelled so that it looks like support.

A producer MUST NOT emit a document describing a configuration surface wider than the binary in the
image actually loads. A workspace with several aggregates has a generator that naturally reaches for
the union of them, and a contract built from that union asserts the image reads a credential no
deployment supplies. Nothing downstream can check this.

## Reading a container

Normative, and an **ordered** list because it has to be — two consumers running these in a different
order disagree about whether a deployment is valid, which is the failure the single wildcard form
exists to prevent, reached through evaluation order instead of pattern syntax.

For each environment variable on a container, first match winning:

1. one of `schema.loader[].env` — a variable the loader reads to decide what the layers are. Valid.
2. some `schema.keys[].env` **or one of that key's `env_aliases`** — that key, from the environment
   layer. Check it in the two steps under [Reading a variable](#reading-a-variable).
3. some `schema.keys[].env_file` **or one of its `env_file_aliases`** — that key, by indirection.
   The value is a path, so neither constraint applies; what applies is that the path is mounted.
4. anything else beginning with `schema.dialect.prefix` — **reject.** A key spelling nothing in the
   image reads. Neither `external.env` nor `external.ignore` can reach this step, because a producer
   refuses both when they carry the prefix.
5. some `external.env[].name` — check it the same two ways.
6. some `external.ignore` pattern — skip it.
7. otherwise — `external.unknown`.

## What the format deliberately cannot say

**A 64-bit range is not checkable from this document.** The largest 64-bit integer is not
representable as an IEEE double, so no `maximum` is published rather than one that is a different
number than the type accepts. Loading the configuration with the real binary is what closes that,
and no arrangement of these fields would.

**A cluster's injected variables cannot be declared.** Kubernetes service links inject
`<SERVICE_NAME>_SERVICE_HOST` and its relatives per Service in the namespace, and the service name
is the release name — which an image cannot know. A release called `portfolio` produces
`PORTFOLIO_SERVICE_HOST` and `PORTFOLIO_PORT` against a `PORTFOLIO_` prefix; a release called
`staging-portfolio` produces names outside it entirely. No declaration written at build time is
right for both.

Set `enableServiceLinks: false` on the pod. It is the deployment's business, and the deployment does
know the release name. It is also not merely a validation nuisance: `PORTFOLIO_PORT` is a spelling
of the key `port`, so with service links on, a Service named after the release *supplies* that key
from the environment layer, outranking the mounted file.

**`unknown: reject` is not free**, for the same reason. A pod carries `HOSTNAME` from the runtime
and `KUBERNETES_SERVICE_HOST` and its relatives from the API server, even on a `scratch` image
running one static binary. Those are what `ignore` is for; relaxing `unknown` instead gives up the
whole gate to tolerate six names.

## Publication

The document is **byte-stable**: the same source tree produces the same bytes, so it can be hashed,
and the hash is what ties three copies of it together. A producer MUST NOT emit a field whose value
varies between runs over one source tree.

Three constants tie a document to an image.

| | Value |
|---|---|
| Artifact type | `application/vnd.terrace.config-schema.v1+json` |
| Label: envelope version | `dev.terrace.config.contract.version` |
| Label: embedded path | `dev.terrace.config.contract.path` |
| Label: prefix | `dev.terrace.config.prefix` |

The registry copy is attached to the digest under that artifact type, and is what a pipeline
fetches: no layer pull, pinned to the digest a chart pins rather than a tag that can move, and
signable. Asking a digest for its referrers of this type returns that digest's contract by
construction, content-addressed by the registry. Nothing means the image does not publish one —
which is an answer, and a different answer from "the contract could not be fetched".

The embedded copy at `dev.terrace.config.contract.path` is what makes an image self-describing with
no registry at all: an exported tarball, an air-gapped mirror, a running container being inspected.

`dev.terrace.config.prefix` is read **before** the document is fetched: it is what tells a validator
which of a pod's variables are this contract's business at all. A consumer SHOULD check it against
the document's own `schema.dialect.prefix`; a mismatch is a build to refuse rather than a copy to
prefer.

There is deliberately **no label carrying the document's hash.** It would buy a cross-check that the
embedded file and the attached artifact are the same document — but that is a failure of the build,
and the build is the one place holding both copies locally and able to compare them for nothing.
Publishing the assertion would make every consumer carry a field none of them need, and it was the
only label that had to be dynamic. Without it all three are constants for a service, which is what
lets them be a plain `LABEL` block with no build argument to interpolate.
