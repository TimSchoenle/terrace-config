# Conformance

What it means for an implementation to produce a contract, and how it is checked.

There are three tiers. They exist because two implementations can agree completely about the
*document* and still disagree about the *spellings*, and can agree about both and still disagree
about the *text*. Collapsing them into one "conforms / does not conform" would either exclude
every implementation that wraps a different binder, or claim an interoperability nobody has.

State your tier. An implementation that claims a tier it does not meet is worse than one that
claims none, because the whole point of the document is that a consumer can act on it without
reading the producer's source.

## Tier 1 — document

Every document the implementation emits:

- validates against [`contract.schema.json`](contract.schema.json);
- satisfies every **MUST** in [`FORMAT.md`](FORMAT.md), including the eight refusals — a producer
  that emits a secret with a default is not tier 1 however well-formed the JSON is;
- carries a `producer` block naming the implementation and the library whose environment reads its
  `text_constraint` patterns were measured against;
- is byte-stable over one source tree.

This is the floor, and it is enough for a consumer to validate a chart's rendered values against
`json_schema`, to check `required` per key, and to classify a container's variables by the ordered
list in `FORMAT.md`. It is what a Spring Boot producer wrapping Spring's own binder can reach
without adopting anything of terrace's naming.

**A tier 1 implementation MUST document its reads.** The per-`text_form` and per-layer tables in
`FORMAT.md` are normative for `figment` and for nothing else. Add your loader's tables to
`FORMAT.md` under a heading of their own, in the same shape, as part of the change that introduces
the implementation. A `producer.loader` value nobody can look up makes step 2 of
[Reading a variable](FORMAT.md#reading-a-variable) unperformable, and a consumer will correctly
skip every range check on your documents.

## Tier 2 — dialect

Tier 1, and: **given the same key paths, the same aliases and the same `dialect`, the
implementation publishes the same spellings.** That is `env`, `env_file`, `secrets_file`,
`env_aliases`, `env_file_aliases`, `secrets_file_aliases` and `unreachable`, for every key.

This is the tier worth aiming at, and the one that costs a decision. Deriving a variable name from
a key path is not the hard part; agreeing about the cases where it *cannot* be derived is:

- a path that does not survive the case fold — `maxAgeSecs` under a lower-casing environment layer
  — is `env: null`, `unreachable: unnameable`, not a name that looks plausible and reaches nothing;
- a path already carrying the nesting separator is the same case;
- a spelling that collides with another key's indirection variable is `unreachable: indirection`,
  and a producer refuses to build.

An implementation that hands naming to a binder with its own relaxed-binding rules will not reach
tier 2 without overriding it, and should not pretend to. Say tier 1 and document the mapping.

The [`unnameable-key`](conformance/unnameable-key/) case exists for this tier specifically.

## Tier 3 — byte

Tier 2, and: **byte-identical output for a shared case**, once `producer.version` is replaced by
the sentinel described below.

Reachable only between implementations sharing a loader *and* a type mapping, because the remaining
fields are derived from both:

| Field | Depends on |
|---|---|
| `text_constraint` | the loader's environment reads, measured |
| `text_form` | mostly the type, but `structured` is the loader's spelling of a list |
| `constraint` | the producer's type vocabulary — an unsigned 16-bit integer has no Java spelling |
| `ty` | the producer's language, by definition |
| `json_schema` | `constraint`, so all of the above |
| `schema.loader[]` | which variables the loader itself reads |

The Rust reference implementation is tier 3 against itself, which is what
[`tests/spec.rs`](../../tests/spec.rs) checks. Do not expect to reach it from another language, and
do not treat failing to as a defect.

## The corpus

```
spec/v1/conformance/<case>/
  README.md        what the case pins, and the source that produces it in each language
  contract.json    the document
  rendered/        what the shared toolchain renders from it — producer cases only
```

Each case is a complete, real document — not a fragment and not a hand-edited one. That is
deliberate: the corpus is the first thing somebody implementing this in another language reads, and
a document with the fields sorted or the awkward parts trimmed teaches the wrong shape.

**One field is substituted.** `producer.version` is `0.0.0-conformance` in every stored document.
It moves on every release and says nothing about the rendering, so leaving it real would make each
release a corpus-wide diff hiding the one line that mattered. Nothing else is normalised: field
order, escaping and whitespace are what the producer actually emits.

### Producer cases

Documents the reference implementation emits, from types it derived them from. Each carries a
`rendered/` directory of goldens with exactly one author: that implementation's `TERRACE_SPEC_BLESS`
run. Everything else that renders is checked against those bytes, and never blesses them — two
renderers that can each rewrite the expectation agree by construction and prove nothing.

| Case | What it pins |
|---|---|
| [`minimal`](conformance/minimal/) | The smallest document that is still a contract: one key, no annotations, no external surface. |
| [`full-surface`](conformance/full-surface/) | Every key field a producer can be asked to fill — secret, note, alias, choice, bounded number, container-of-choice, nested struct, required key, reserved loader variable, declared and ignored externals. |
| [`unnameable-key`](conformance/unnameable-key/) | A key no variable can name, and the `unreachable` reason that says which kind. |

### Consumer cases

The other half, and the half a producer-only corpus cannot hold: documents a **consumer** has to
survive. No producer in this repository can emit any of them — that is what they are for — so they
are hand-written, carry no `rendered/`, and are held by assertions about what a reader must do
rather than by goldens.

They are the cheapest tests here and the ones that catch a second implementation's integration
breaking months before there is a second implementation's document to vendor.

| Case | What it pins |
|---|---|
| [`foreign-loader`](conformance/foreign-loader/) | A `producer.loader` with no measured read table: step 2 skipped and reported, never silently performed. |
| [`schema-version-1`](conformance/schema-version-1/) | A `schema_version` below the current one: a reader degrades to "no element schema published", and a generator narrows rather than refusing. |
| [`tier-1-spellings`](conformance/tier-1-spellings/) | Spellings that do not follow from the dialect: no rule re-derives a name the document states. |
| [`foreign-ty`](conformance/foreign-ty/) | A type vocabulary from another language: `ty` is printed and never matched on, so removing it changes no finding. |

### Running it

The reference implementation checks itself on every test run:

```bash
cargo test --all-features --test spec
```

Four checks, failing for different reasons. Two validate what the crate renders *now* against the
meta-schema — one for the whole envelope, one for the `schema` half on its own, since that half is
published as an artefact in its own right. One validates what was *blessed*, which is the failure a
corpus alone cannot catch: an expectation stored while wrong. One compares the two.

When a rendering changes on purpose:

```bash
TERRACE_SPEC_BLESS=1 cargo test --all-features --test spec
```

Then read the diff. That diff is the change every other implementation now has to match, and it
belongs in the pull request that caused it rather than in a follow-up.

Another implementation checks itself the same way, against the same files: build the case's fixture
in your language, render it, substitute `producer.version`, and compare against the fields your tier
claims.

## Adding an implementation

1. **Pick `producer.name` and `producer.loader`.** The first identifies your code, the second the
   library whose environment reads you are about to measure. Two implementations wrapping the same
   library share the second and not the first.
2. **Measure the reads.** Not derive them — measure. Feed the binder `+5`, `007`, `1_000`, `0x1F`,
   `1e3`, `TRUE`, `yes`, `1`, values with leading and trailing whitespace, and a list in whatever
   spellings it might take, and write down what it accepts. Every pattern you publish must be a
   superset of what you measured; a pattern narrower than the loader stops a deployment that was
   correct.
3. **Add your tables to `FORMAT.md`** in the shape the figment ones take.
4. **State your tier** in your own documentation, and where you diverge if it is tier 1.
5. **Add your fixtures** for the cases your tier covers, and wire them into your build the way
   `tests/spec.rs` is wired into this one.

## Open questions

Recorded rather than guessed at. Each one is a place the format may need to grow when a second
implementation exists to measure against; inventing the answer now would mean inventing a
vocabulary from one data point.

**How a `structured` key is spelled in one variable.** `figment` takes a TOML literal, so
`PORTFOLIO_GITHUB__REPOS=["a","b"]` loads and `a,b` does not. Spring's relaxed binding takes
comma-separated values, and also takes indexed names — `..._REPOS_0_`, `..._REPOS_1_` — which are
*variables this format has no key for at all*. The second is the real problem: step 4 of
[Reading a container](FORMAT.md#reading-a-container) rejects a variable carrying the prefix that
matches no key, so a correct Spring deployment would fail the gate. Neither `text_form` nor
`text_constraint` can express "and also every name of this shape".

The likely answer is a per-loader statement about indexed spellings rather than a pattern language,
but it should be written against a real binder, not against this paragraph.

**Whether `text_form` needs a value for a fixed-point or decimal type.** A 64-bit float is
`text_form: unknown` in the reference implementation — no measured pattern describes what its parse
accepts — and a Java implementation over `BigDecimal` may be able to say more.

**Whether `producer` should carry the loader's version.** The reads are measured against a specific
release, and a binder that relaxes its parsing in a minor version silently invalidates every pattern
measured before it. Against that: a version here would be a third version string in one document,
and nothing consumes it yet.
