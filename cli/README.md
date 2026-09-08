# terrace-contract

Everything the configuration contract needs *after* a document exists.

A service publishes one JSON document describing every setting its binary reads —
[`spec/v1/`](../spec/README.md) is what that document is. Producing one needs the service's own
types and can only be per-language. Everything downstream of it needs a document and nothing else,
and that is this binary.

```
per language, once                    this binary, for everyone
───────────────────────────────       ─────────────────────────────────────────
types ──> Schema ──> Contract ──JSON──> render    the tables, the file, the labels
                                        stamp     build identity onto a document
                                        conform   the refusals, and a tier
                                        validate  the published meta-schema
                                        image     labels and Dockerfile block, read back
```

## The point

**A new implementation's obligation is one sentence: emit a document that passes
`terrace-contract conform`.** Not a renderer per format, not a validator, not a test kit.

That is what makes "and any future language" a claim rather than a hope. Every rendering below is a
pure function of the published document — nothing in a markdown table or a TOML skeleton needs
Rust's types, figment, Jackson or Spring — so a renderer that reads a document runs for every
producer, including ones nobody has written yet.

`spec/v1/conformance/<case>/rendered/` is what holds that honest. Those bytes are blessed by the
Rust implementation, from the types it derived the document from; this binary renders the same
bytes from the document alone, and [`tests/corpus.rs`](tests/corpus.rs) fails if the two ever
disagree.

## Usage

```bash
terrace-contract render --format markdown contract.json >> README.md
terrace-contract render --format toml contract.json > config.example.toml
terrace-contract render --format json-schema --title "portfolio configuration" contract.json
terrace-contract render --format dockerfile contract.json

terrace-contract conform --tier 2 contract.json
terrace-contract validate contract.json

terrace-contract stamp --app-version "v2.5.0" --revision "$GIT_SHA" contract.json
terrace-contract image verify --labels labels.json --dockerfile Dockerfile contract.json
```

Every command reads a document from a path or from `-`, so the producing step and the rendering
step compose without a temporary file between them:

```bash
./gradlew -q terraceContract | terrace-contract conform --tier 1 -
```

| Format | What it is |
|---|---|
| `json` | the schema half, for a pipeline that renders its own tables |
| `markdown` | both tables, for a README |
| `markdown-loader` | the loader's own variables alone |
| `markdown-keys` | the configuration keys alone |
| `toml` | the commented file an operator copies to `config.toml` |
| `json-schema` | a JSON Schema, for an editor to validate that file against |
| `contract` | the whole document, which is what a build embeds in its image |
| `labels` | the image labels that make it discoverable, one `NAME=value` per line |
| `dockerfile` | the same labels as a marked `LABEL` block to paste |

Exit `0` clean, `1` read it and found it wanting, `2` could not read it. A pipeline that conflates
the last two cannot tell a failing gate from a broken one.

## In a Docker build

The contract-producing stage is the only language-specific part:

```dockerfile
FROM ghcr.io/timschoenle/terrace-contract:1 AS contract
COPY --from=contract-builder /out/contract.json /in/contract.json
RUN terrace-contract conform --tier 2 /in/contract.json \
 && terrace-contract render --format labels /in/contract.json > /out/labels
```

A Spring Boot service's Dockerfile is that same file with a different `contract-builder` stage.

## What it deliberately does not do

- **Produce a contract.** That needs the types. It is the seam.
- **Depend on any implementation.** Not on `terrace-config`, not transitively. CI asserts the
  absence, because a renderer reaching for one producer's types would be that producer's renderer
  wearing a shared name — and nothing else in this repository would notice that edge appearing.
- **Guess at a loader it does not know.** `producer.loader` names the library whose environment
  reads a document's `text_constraint` patterns were measured against. A consumer meeting an
  unknown one must skip the read that depends on them and say so, rather than perform it with the
  wrong rules.

## Building

Its own Cargo workspace, and a sibling of [`../src`](../src) rather than a member of it — the
binary is not part of the Rust implementation, it is *written in* Rust.

```bash
cd cli
cargo test
cargo clippy --all-targets --no-default-features   # the half a language's build links
```

`--no-default-features` leaves the producer-side half: the document model, the renderers, `conform`,
`stamp` and `image`. Keeping it compiling is the mechanical check that no consumer-side concept —
a chart, a manifest, a Helm marker — has leaked down into it.

See [`docs/contract-cli-plan.md`](../docs/contract-cli-plan.md) for what is built and what is not.
