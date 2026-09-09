# `schema-version-1`

The version a new producer implements first: `constraint` before it learned to nest.

Nothing was removed between 1 and 2 and nothing changed meaning — 2 added the ability for a
container-typed key to carry its *element*'s schema under `items` or `additionalProperties` — so a
version-1 document is a version-2 document that happens to nest nowhere.

## What it pins

- `peers` is a map and `methods` is a sequence, and **neither says what one element holds**. A
  reader must degrade to "no element schema published" rather than assume `items` is there and read
  past the end of what the producer said.
- A generator that wants an element schema must **narrow** rather than refuse. A document that says
  less is not a document that is wrong.
- The value checks are unaffected: `structured` has no range step under any loader, because a TOML
  literal's contents are beyond what a flat constraint describes.

## What is not pinned yet

The `shape` and `probe` rules that read an element schema arrive with the marker language and the
derived documents. Until they do, this case asserts the reads that exist: the document is readable,
conforms at tier 1, and satisfies the published meta-schema at a version below the current one.

## Source

Hand-written, and modelled on the one non-Rust document a consuming repository already carried —
which was `schema_version: 1`, and which was also **not valid** against
`spec/v1/contract.schema.json`, because it carried no `producer` block and the schema requires one.
Nothing noticed, because nothing read the field. Correcting it is what this case is.
