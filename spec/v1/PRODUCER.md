# Writing a producer

How to publish configuration contracts from a language that has no implementation yet.

**Status: a guide.** [`FORMAT.md`](FORMAT.md) defines the document and
[`CONFORMANCE.md`](CONFORMANCE.md) what an implementation is held to; this file is the route
through both for somebody starting from nothing, and defers to them where the three differ.

A chart can only gate on an image that publishes a contract, so every language without a producer
is a set of images no chart can enrol. This guide exists to make that gap cheap to close, and the
claim it rests on is one sentence:

**A new implementation's whole obligation is to emit a document that passes
`terrace-contract conform --tier <n>`.**

Not a renderer per format, not a validator, not a test kit, not a label block. Every rendering,
every image check and the normative copy of every refusal belong to the shared binary, and are the
same code for every producer, including the ones nobody has written yet.

| | Yours | `terrace-contract`'s |
|---|---|---|
| Reading your language's types | yes | |
| Deriving keys, spellings, constraints, defaults | yes | |
| Writing the JSON document, byte-stably | yes | |
| Measuring your loader's reads, and documenting them | yes | |
| The twelve refusals | optional, as a fast path with better messages | normative: `conform` |
| The meta-schema | | `validate`, and `conform` before its rules |
| Tables, `config.example.toml`, JSON Schema, labels | | `render` |
| Build identity | | `stamp` |
| The Dockerfile label block, the image read-back, the referrer | | `render --format dockerfile`, `image` |

[`producers/python/`](producers/python/) is the proof: a derivation module and an example service,
under 400 lines of Python together and using only the standard library, emitting a document
`conform --tier 2` and `validate` accept. Read it beside this file.

## Before you start

Three decisions, each recorded in the document or beside it.

1. **`producer.name`**, a stable identifier for your code. Not a display name, and not the name of
   the library it wraps.
2. **`producer.loader`**, the library whose environment reads the document's `text_constraint`
   patterns were measured against. Two implementations wrapping one library share this and not the
   first. A consumer applies the reads in [Reading a variable](FORMAT.md#reading-a-variable) only for
   a loader it recognises, so this value decides how much of your document is checkable at all.
3. **The tier you will claim.** See [Picking a tier](#picking-a-tier); it decides how you derive
   every spelling, and it is cheaper to decide before the first key than after the hundredth.

## What to emit

One JSON document. The corpus under [`conformance/`](conformance/) holds complete real ones;
[`full-surface`](conformance/full-surface/) is the case to work through field by field.

### The envelope

```json
{
  "terrace_contract": 1,
  "producer": { "name": "…", "version": "…", "loader": "…" },
  "app":      { "name": "…", "version": "…" },
  "schema":   { "schema_version": 2, "dialect": { … }, "loader": [ … ], "keys": [ … ] },
  "json_schema": { "$schema": "http://json-schema.org/draft-07/schema#", … },
  "external": { "env": [ … ], "ignore": [ … ], "unknown": "reject" }
}
```

All six members, always. `terrace_contract` is `1` for everything this directory describes.

### `producer` and `app`

`producer.version` is your implementation's version, not the service's. It is the one field the
corpus substitutes (with `0.0.0-conformance`) when comparing, so it may move every release without
moving anything else.

`app.name` is required; `version`, `revision`, `created` and `source` are optional. Emit what the
source tree knows and leave build identity to the build: `terrace-contract stamp --app-version
--revision --created --source` writes those fields onto a finished document and changes nothing
else, so a producer that cannot see a tag or a commit does not have to invent one. Nothing in the
document names the image; [Publishing into an image](#publishing-into-an-image) is where that tie
is made.

### `schema`

**`schema_version`.** The lowest version whose vocabulary holds every keyword you emit
([Versioning](FORMAT.md#versioning)). Versions 1 and 2 read identically for a document whose
constraints nest nowhere, and both existing implementations write 2 as their floor; do the same,
and raise it only when you emit a keyword of version 3 (`propertyNames`) or 4 (the conditions
between a struct's fields).

**`dialect`.** `prefix` (never empty: refusal 7), `nesting_separator` and `indirection_suffix`.
These are your loader's, stated, not chosen to match anybody else's.

**`loader[]`.** The variables your loader reads to decide what the layers *are*: the one naming the
document, the one naming a secrets directory, any it reads before the layers exist. Each with a
`role`, `docs` and `default`. Publish only variables the loader really reads: a consumer accepts
every name here unconditionally (step 1 of [Reading a container](FORMAT.md#reading-a-container)).

**`keys[]`.** One entry per leaf of the configuration, `path` unique. Every member below is required
by the meta-schema except `constraint`, `text_constraint`, `unreachable` and `reload`.

| Field | Where a producer gets it |
|---|---|
| `path` | Field names, `.`-joined through nested tables, as the document spells them. |
| `env`, `env_file`, `secrets_file` | The path and the dialect. See [Picking a tier](#picking-a-tier) for the derivation. |
| `docs` | Whatever your language annotates fields with: doc comments, an annotation, field metadata. Empty when there is none. |
| `ty` | Your language's own spelling of the type, optionality stripped. Printed, never matched on ([`foreign-ty`](conformance/foreign-ty/)). |
| `values` | The members of a choice type, as the loader accepts them. Empty otherwise. |
| `constraint` | What the type certainly guarantees, as JSON Schema. Omit it rather than guess. |
| `text_constraint` | A pattern over the raw variable text, **measured** against the loader. Omit it when you measured nothing. |
| `text_form` | `text`, `integer`, `boolean`, `choice`, `structured` or `unknown`. Always present. |
| `aliases` and the three alias lists | Other spellings that load, each through the same derivation. Empty when there are none. |
| `unreachable` | `unnameable` when no spelling names the key. Required whenever `env` is `null`. |
| `default`, `default_value` | The default instance: as display text, and as a JSON value. `null` for a secret, whatever the type holds. |
| `note` | Prose qualifying a default, or `null`. |
| `required` | Whether some layer must supply the key; in practice, no default and not optional. Not JSON Schema's `required`. |
| `secret`, `reserved` | Whether the value is a credential; whether the loader reads it before the layers exist. |
| `reload` | Only with `schema.reload`. Absent means undeclared, which reads as `restart`. |

Two of these carry most of the risk, and in opposite directions. `constraint` must never be looser
than the type: a value the type accepts and the constraint rejects is a correct deployment refused.
`text_constraint` must be a superset of what the loader accepts, for the same reason. Where you
cannot be certain, the field is absent, and a consumer that meets the absence says it skipped the
check rather than passing it.

**A secret never carries a default** (refusal 6). The type may well hold one, for local development;
the document is about to enter a public registry. Drop it at derivation time, as the reference does,
rather than relying on `conform` to catch it after the fact.

### `json_schema`

The same keys as a draft-07 schema, nested at each key's position in the document, for a chart to
validate the file it renders. Closed (`additionalProperties: false`) at every level by default,
carrying each key's `constraint`, its `docs` as `description` and its default as `default`, every
alias as a property of its own, and no `reserved` key. It carries **no** `required` list
([The `json_schema` half](FORMAT.md#the-json_schema-half)): a required secret arrives through a
mount, so the rendered file correctly omits it. Both existing implementations also mark a secret
`writeOnly`.

### `external`

Variables the image reads that are not configuration: a runtime's `PORT`, a TLS library's
`SSL_CERT_FILE`. Declared in `env[]` with the same two-space fields a key carries, and checked like
one; names nobody owns in `ignore[]`, where only a trailing `*` is a wildcard; everything else falls
to `unknown`, which is `reject` unless there is a reason. No producer can discover these, because
none carries the prefix, so they are a list the service's author writes by hand. Refusals 1 to 5 are
all about this half: nothing here may reach into the prefix or the loader's own variables.

### What to refuse

[What a producer MUST refuse](FORMAT.md#what-a-producer-must-refuse) lists twelve documents no
producer may emit. `conform` checks every one of them and is the normative copy, so a producer whose
build runs it is correct without reimplementing any. Two are worth deciding at derivation time
anyway, because only the producer holds the information to explain them: a secret's default (drop
it), and a key whose spelling is another key's indirection variable (`token` beside `token_file`;
refusal 8), which the reference refuses with both key paths in the message.

## Picking a tier

[`CONFORMANCE.md`](CONFORMANCE.md) defines three. Claim the highest one you meet and no higher: a
consumer acts on the document without reading your source, and a claim it cannot trust is worse
than none.

| Tier | Means | What `conform --tier <n>` runs |
|---|---|---|
| 1, document | Well-formed, every refusal satisfied, the loader's reads documented. | The meta-schema, then the twelve refusals. |
| 2, dialect | Tier 1, and every spelling is the one the dialect derives. | Tier 1, and `env` and `env_file` re-derived for every key. |
| 3, byte | Tier 2, and byte-identical to another implementation for a shared case. | Tier 2. Byte-identity is a property of two documents, so `conform` accepts tier 3 without a further check. |

**Tier 1** is reachable from any binder. A producer that hands naming to a framework with its own
relaxed binding publishes the names that framework reads, and stops here honestly. It owes one
thing more than a valid document: its loader's read tables, added to `FORMAT.md` in the shape the
`figment` ones take ([Adding an implementation](CONFORMANCE.md#adding-an-implementation)). Without
them, every consumer correctly skips the range step on its documents.

**Tier 2** is the one to aim at. It means the spellings follow from the path by exactly this
derivation, and the cases where nothing derives are agreed rather than guessed:

1. A path already containing `nesting_separator` names nothing.
2. Otherwise the name is the path with every `.` replaced by `nesting_separator`.
3. The name is nameable only if upper-casing it, lower-casing the result and turning each
   `nesting_separator` back into `.` gives the path back. The environment layer folds names to
   lower case on the way in, so `maxAgeSecs` does not survive the trip.
4. When it is nameable: `env` is `prefix` followed by the name upper-cased; `env_file` is `env`
   followed by `indirection_suffix`; `secrets_file` is the name itself.
5. When it is not: all three are `null`, and `unreachable` is `unnameable`, unless an alias names
   the key, in which case it is not unnameable at all
   ([`unnameable-key`](conformance/unnameable-key/), *The trap*).
6. Every alias is derived the same way, into `env_aliases`, `env_file_aliases` and
   `secrets_file_aliases`.

`conform` re-derives `env` and `env_file` for every key, and requires `unreachable` beside every
`env: null`. The rest of the tier is yours to check against the corpus: `secrets_file`, the alias
lists, and which reason `unreachable` gives. Build the fixture each producer case describes in your
language, and compare the spellings field by field. The reference's
[`test_producer.py`](producers/python/test_producer.py) does exactly this for `minimal` and
`unnameable-key`.

**Tier 3** needs a shared loader *and* a shared type mapping, because `ty`, `constraint`,
`text_constraint` and `json_schema` follow from both. A new language shares neither with Rust, and
not reaching it is not a defect. Keep a golden of your own instead, as `java/` does under
`terrace-config-spec-tck/src/test/resources/conformance-java/`, and you are tier 3 against yourself.

## Byte stability

The same source tree produces the same bytes ([Publication](FORMAT.md#publication)). The document is
hashed, and compared byte for byte between the image's embedded copy and its referrer, so a
document that moves on its own is one that fails verification for no reason anybody can see.

- **Fix every order in the source.** Keys in declaration order, members of each key in the order the
  corpus writes them, `json_schema` members sorted by name. Never iterate a hash map or a set to
  produce output.
- **Read nothing variable.** Not the clock, not the host name, not the working directory, not an
  environment variable. Build identity arrives through `stamp`, from values the build states.
- **Write bytes, not text.** UTF-8, one line ending, a fixed indent, and a fixed rendering for every
  number. A platform's text mode translating `\n` is a different document on Windows.
- **Prove it.** Commit the document and have a test regenerate and compare it, so a change to the
  types that changes the document is a reviewed diff rather than a surprise in the registry. The
  reference also regenerates it under three hash seeds, the one input a Python process varies by
  itself.

Indent and member order are not normative: a consumer parses the document. But a document whose
bytes survive the shared model unchanged is one `stamp` will not reformat, so the stamped copy in
the image differs from the one in the repository in its identity fields and nowhere else. Check it
once:

```bash
terrace-contract render --format contract contract.json | cmp - contract.json
```

## Testing

```bash
terrace-contract conform --tier 2 contract.json   # the obligation
terrace-contract validate contract.json           # the meta-schema alone
```

Both read a path or `-` for stdin, so a producing step pipes straight in. Both print nothing and exit
`0` on success, `1` with one line per finding when the document is wanting, and `2` when it could
not be read at all. A finding names a JSON pointer, a sentence and the rule:

```text
the document does not conform at tier 2:
  /schema/keys/1: `port` is spelled `ORDERS_PORTX`; this dialect derives `ORDERS_PORT` (tier 2)
```

`conform` runs the meta-schema first and stops if it fails, since the rules below it assume a
well-formed document; `--no-schema` runs only the rules. `validate` is the meta-schema and nothing
else, which is the check for a document somebody else handed you.

Then the corpus. The producer cases under [`conformance/`](conformance/) each carry the source that
produced them; express the ones your type vocabulary can in your language, and compare against the
fields your tier claims. The consumer cases are not for you to reproduce: they are documents a
reader has to survive, and two of them ([`foreign-loader`](conformance/foreign-loader/),
[`foreign-ty`](conformance/foreign-ty/)) describe exactly how a reader treats a new producer's
document.

To see the document as an operator will, render it:

```bash
terrace-contract render --format markdown contract.json
terrace-contract render --format toml contract.json
```

## Publishing into an image

A contract reaches a consumer through the image it describes, in two carriers and three labels:

| | Where | Value |
|---|---|---|
| Embedded file | inside the image, at the labelled path | the document's bytes |
| Referrer | attached to the image's digest in the registry | the document's bytes, artifact type `application/vnd.terrace.config-schema.v1+json` |
| Labels | the image config | `dev.terrace.config.contract.version`, `dev.terrace.config.contract.path`, `dev.terrace.config.prefix` |

**At least one carrier is required, and when both exist they must be byte-identical.** A chart
repository's `pull` reads both and refuses an image whose two copies disagree. A registry with no referrers API carries
the embedded file alone, and that is a complete publication: the file lives in a layer whose digest
is in the manifest the chart pins, so it is provably inside the image, which an attached artifact
merely pointing at the same digest is not. The referrer is the faster read, not the stronger one.

None of this is per-language. The steps below are the same for every producer, and the only one
that differs is the stage that runs your producer.

### The label block

The labels are constants for a service, so they are a committed `LABEL` block rather than build
arguments. Place the marker pair once, where the block belongs:

```dockerfile
# terrace-config:labels:begin
# terrace-config:labels:end
```

and let the binary fill it:

```bash
terrace-contract render --format dockerfile --write Dockerfile contract.json   # rewrite in place
terrace-contract render --format dockerfile --check Dockerfile contract.json   # CI: exit 1 when behind
```

`--write` rewrites every `# terrace-config:labels:begin`/`:end` region in the file, since a
Dockerfile building several images carries one per stage; `--check` exits `1` when any region
differs from what the document renders. A Dockerfile without the marker pair is refused rather than
edited: where the block goes is the author's decision, made once.

The block names `/config/contract.json` as the embedded path; `--path` names another. Copy the
document there in the image's final stage, from the stage that ran your producer, with the same
bytes `conform` accepted.

### Verifying the built image

```bash
terrace-contract image verify contract.json --image registry.example/orders@sha256:…
terrace-contract image verify contract.json --image registry.example/orders@sha256:… \
  --signer 'https://github.com/example/orders/\.github/workflows/release\.yaml@refs/heads/main'
terrace-contract image verify contract.json --oci-layout build/oci:orders
```

`image verify` reads the image's labels and the embedded file at the labelled path itself, from the
registry through `oras`, and requires the embedded bytes to equal the document given. For an index,
every platform must agree. When a referrer is attached it is checked too, and must be
byte-identical. `--signer <REGEX>` additionally verifies the image's signature through `cosign`
against that identity, which is the check a chart repository's `pull` will make. `--oci-layout`
reads `REF` as an OCI image layout directory, `<dir>[:<tag>|@<digest>]`, instead of a registry, so
an image that is built but not yet pushed can be verified before it is.

The document given is the one the image should carry: if the build stamps it, verify against the
stamped bytes.

### Attaching the referrer

```bash
terrace-contract image attach contract.json --image registry.example/orders@sha256:…
```

Attaches the document to the image's digest as an OCI referrer of artifact type
`application/vnd.terrace.config-schema.v1+json`. Attach the same bytes the image embeds, after the
push, and verify afterwards: the verification is what holds the two carriers to each other.

### In GitHub Actions

The composite action `TimSchoenle/actions/actions/terrace/image` runs these steps in order:
`conform` at a tier, the Dockerfile check, `image verify`, and optionally `image attach` and keyless
`cosign` signing. Its README documents the inputs.

## Checklist

- `producer.name`, `producer.loader` and the tier are decided and written down.
- The loader's reads are measured and its tables added to `FORMAT.md`. The reference is the one
  document without them, because it has no loader at all; its README's *Reads* says what that
  costs a consumer.
- `conform --tier <n>` and `validate` run in the producer's own build, on the document it emits.
- The document is committed and regenerated by a test, byte for byte.
- The producer cases your types can express are fixtures, compared on the fields your tier claims.
- The Dockerfile carries the marker pair, and CI runs `render --format dockerfile --check`.
- The image embeds the document, `image verify` holds it, and a referrer, if attached, carries the
  same bytes.
