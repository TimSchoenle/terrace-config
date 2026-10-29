# Architecture

How terrace-config publishes a service's configuration surface with its image, and how a chart
repository proves that what it renders is configuration the pinned image accepts.

The normative definitions live in [`spec/`](../spec/README.md). This document explains how the
pieces fit and why they are shaped the way they are. Where the two disagree, `spec/` is right.

## The defect it closes

A chart renders a service's `config.toml` from its own values. Nothing in a stock chart pipeline
checks that a rendered key is still one the service reads:

- `helm template` renders, and `values.schema.json` describes the chart's values, not the service's
  configuration.
- `kubeconform` passes, because a ConfigMap holding arbitrary text is a valid ConfigMap.
- `helm lint`, `kube-linter`, `helm unittest` and an install all pass.
- The pod starts on the compiled default, because `serde` ignores an unknown key by design.

The same gap exists on every other surface a chart writes: an environment variable whose key was
renamed is a silent no-op, a secret file whose key was renamed is never read, and a key supplied by
two layers at once is a boot failure that shows up only as a `CrashLoopBackOff`.

Every one of these is knowable at render time, because the loader computes all of it from the
service's types. The design carries that knowledge from the image to the job that renders the chart:
**a digest-addressed document attached to the image**, vendored into the chart repository, and
checked against every render.

## Components

```text
spec/     the format, the conformance corpus and its rendered goldens, and the Helm vocabulary
rust/     the Rust implementation: loader, producer, derive macro, fuzz targets
java/     the Java implementations: annotations, processor, core, loader, Spring Boot, Gradle plugin
cli/      terrace-contract, the shared toolchain; its own Cargo workspace, depending on no implementation
docs/     cross-language documents, this one included
```

`cli/` is a sibling of `rust/`, not a member of it. The binary is written in Rust, but it is not part
of the Rust implementation, and CI asserts with `cargo tree` that `cli/` never depends on
`terrace-config`.

### The seam

```text
per language, once                    terrace-contract, for every language
───────────────────────────────       ────────────────────────────────────────────────
types ──> Schema ──> Contract ──JSON──> render   markdown, toml, json-schema, labels, …
                                        stamp    build identity onto a document
                                        conform  the refusals, and a conformance tier
                                        image    labels and the Dockerfile block, read back
                                        check    a rendered chart, against the document
                                        …
```

Everything left of the arrow needs the language's toolchain and is per-language. The document,
rendering and conformance layer right of it needs a JSON document and nothing else; the image and
chart commands also need the image, a registry or a rendered chart, but never the producer's
language. A new implementation's obligation is therefore one
sentence: **emit a document that passes `terrace-contract conform --tier <n>`.** It writes no
renderer, no validator and no conformance kit. Passing `conform` is the document's half of a tier,
not the whole claim: tier 1 also needs the implementation's reads documented in
`spec/v1/FORMAT.md`, and tier 3 needs its fixtures to match the corpus goldens byte for byte (see
"Adding an implementation" in `spec/v1/CONFORMANCE.md`).

| | Rust | Java | Another language |
|---|---|---|---|
| types to descriptors | `Describe` derive | `terrace-config-processor` | its own |
| descriptors to `Contract` | `Schema::into_contract` | `ContractAssembler` | its own |
| `Contract` to JSON | `Contract::to_json` | `ContractCodec` | its own |
| renderings | the library keeps its own, held to the goldens | none | none |
| refusals | a fast path with better messages | a fast path with better messages | optional |

Rust keeps its renderers because its `schema` feature has a runtime audience: a service rendering its
own key table at boot, which a build-time binary cannot serve. The rendered goldens stop the two
copies drifting (see [Renderings](#renderings-are-artefacts-of-the-spec)).

## The contract document

### One envelope

The loader's schema renders two machine-readable halves, and neither subsumes the other: the key
table can validate an environment variable name or a secrets file name, and the JSON Schema can be
handed to a stock validator. They ship as one document, so there is one hash, one fetch and no way
for half of it to be stale:

```json
{
  "terrace_contract": 1,
  "producer": { "name": "terrace-config", "version": "…", "loader": "figment" },
  "app": { "name": "orders-service", "version": "v2.5.0", "revision": "…", "created": "…" },
  "schema": { "schema_version": 2, "dialect": {}, "loader": [], "keys": [] },
  "json_schema": { "$schema": "http://json-schema.org/draft-07/schema#" },
  "external": { "env": [], "ignore": ["KUBERNETES_*"], "unknown": "reject" }
}
```

`terrace_contract` is the envelope's version, independent of `schema_version` inside it, and a
consumer that does not recognise it refuses the document by name. Every field is `snake_case`.

There is no `app.digest`. A digest is what building the image produces, so a field carrying it would
have to be written after the push and would change the bytes. The registry attachment is the tie
instead: a document fetched as a referrer of a digest already names its image.

### Determinism

The document is hashed, committed and diffed, so it is byte-identical across runs. The producer reads
nothing from the environment, keys keep declaration order, and objects are serialised sorted.
`app.created` and `app.revision` are the only fields that move between builds of one source, and they
live in `app`, never inside `schema` or `json_schema`, so a consumer can diff the half that describes
the configuration and ignore the half that describes the build.

A service repository commits its own copy without `--revision` and `--created` and fails CI on
drift. A removed key then shows up in the diff of the pull request that removes it.

### `external`: the surface no derive can see

A service also reads variables that are not its configuration: a toolchain's `PORT`, `IP` and
`RUST_LOG`, a base image's `PATH`. None carries the loader's prefix, so no type description can
report them, and a gate that flagged everything it could not account for would flag all of them.

`external` is a positive declaration, not a suppression list:

| | Says | The gate |
|---|---|---|
| `external.env` | this image reads the variable, with these constraints | checks it like a key, as far as its constraints allow |
| `external.ignore` | nobody here owns it | skips it; only a trailing `*` is a wildcard |
| `external.unknown` | what happens to everything else | `reject` by default |

The producer refuses every declaration that would let an application remove a real key from the gate
that owns it: an external variable carrying the loader's prefix, an ignore pattern reaching into that
namespace or covering a spelling the loader reads, an external variable colliding with a loader
spelling or declared twice, an empty prefix, a key whose environment spelling is another key's
`_FILE` variable, and a secret carrying a default. [`spec/v1/FORMAT.md`](../spec/v1/FORMAT.md) lists
them as the refusals.

Keys answer to their `serde` aliases in every layer, so the document publishes `env_aliases`,
`env_file_aliases` and `secrets_file_aliases` beside the canonical spellings, and every gate accepts
them. An alias is how a maintainer keeps deployments working across a rename, and a gate refusing
the old spelling would fail the one thing that made the rename safe.

### Constraints, so a consumer in any language can read it

`ty` is the producer's own type name and carries no portable meaning. Every key and every declared
external variable therefore carries its constraints as JSON Schema keywords, twice, because a value
exists in two forms:

```json
{ "path": "isr.ttl_secs", "env": "ORDERS_ISR__TTL_SECS", "ty": "u64",
  "text_form": "integer",
  "constraint":      { "type": "integer", "minimum": 0 },
  "text_constraint": { "type": "string", "pattern": "^\\s*\\+?[0-9]+\\s*$" } }
```

In a TOML file the value is an integer; in the environment it is a string. `constraint` checks the
first and `text_constraint` the second. The text patterns are measured against the loader rather
than derived from TOML's grammar, and each is a superset of what was measured, because a pattern
refusing text the loader accepts stops a correct deployment. `text_form` names the parse a consumer
applies before the range check, and it separates "any text is fine" (`text`) from "nothing could be
determined" (`unknown`).

A secrets file and a `_FILE` target deliver strings, and the loader does not coerce them, so a key
whose `constraint` is not a string type cannot be supplied by either. That is the rule the Rust
guidance states ([`CONTRACT.md`](../rust/docs/CONTRACT.md)), and it admits a string key whose
environment parse is `text_form: unknown`, such as an address. Gate 3 is deliberately stricter: it
accepts only `text_form: text`, because nothing says how a loader reads an `unknown` key, and a
false report on a mount costs one review comment where a missed one leaves a credential unread.

### What the document cannot say

Kubernetes service links inject `<SERVICE>_SERVICE_HOST`, `<SERVICE>_PORT` and more for every
Service in the namespace. The service name is the release name, so the variables land inside the
prefix or outside it depending on how the release is named, and an image cannot know that at build
time. A chart that adopts the gates sets `enableServiceLinks: false`. With links on, a Service named
after the release can supply a configuration key from the environment layer, which is a live
misconfiguration and not merely a validation nuisance.

## Where the document lands on the image

Three carriers, each answering a different question.

**Labels, for discovery.** In the image config, readable with one request and no layer pull:

```dockerfile
LABEL dev.terrace.config.contract.version="1" \
      dev.terrace.config.contract.path="/config/contract.json" \
      dev.terrace.config.prefix="ORDERS_"
```

They are constants, so `terrace-contract render --format dockerfile` writes them into a marked block
of the Dockerfile and `terrace-contract image verify` checks the built image, not the source. A
source diff cannot see a label a base image overrode or a build argument that failed to interpolate.

**A file in the image, the offline copy.** `COPY --from=contract-builder /out/contract.json
/config/contract.json` makes the image self-describing without a registry: an exported tarball, an
air-gapped mirror, or a registry without the referrers API.

**An OCI referrer, the canonical fetch.** `terrace-contract image attach` attaches the document to
the pushed digest with artifact type `application/vnd.terrace.config-schema.v1+json`, and the build
signs the referrer with `cosign sign`. The referrer is attached to the exact digest a chart pins,
leaves the image untouched, costs two small requests, and is signable.

`terrace-contract pull` reads both the referrer and the embedded file. At least one must be present,
and if both are, they must be byte-identical. The image's `dev.terrace.config.prefix` must equal the
document's `schema.dialect.prefix`, and a mismatch is a hard error.

## The toolchain

### Layout and features

```text
cli/src/
  document.rs   the envelope, read tolerantly, gating on `terrace_contract` before anything else
  render/       one module per rendering, all from a document
  conform.rs    the refusals, and tiers 1 to 3
  validate.rs   a document against the embedded meta-schema
  union.rs      several images' contracts, one document
  classify.rs   the ordered classification of an environment variable
  value.rs      the per-loader value checks
  diff/         findings between two documents, and their impact
  oci/          the registry, by delegation to `oras` and `cosign`
  k8s.rs        manifests, workloads, containers, mounts
  gate/         the document, container and requiredness gates
  helm/         the chart repository's conventions
  bin/          argument parsing, dispatch and exit codes
```

The features stack and the arrow points one way: `cli` → `helm` → `k8s` → the core, with `oci`
beside the core. `--no-default-features` leaves the producer-side half, which is what a language's
build needs and which must never learn what a chart is. CI builds that configuration, which is the
mechanical check that no Helm concept has leaked into the document model, the renderers or the value
checks.

`oci` shells out to `oras` and `cosign` rather than linking a registry client. Both are already
pinned by the repositories that call it, and a registry client would carry a TLS stack, a
credential-helper protocol and a signature implementation into a binary whose other subcommands touch
no network.

Charts are read with two passes over each YAML file that meet on a line number: `saphyr-parser`
events give every mapping key's dotted path and line from the document's own structure, and a
line scan gives every `@config` marker, because no Rust YAML crate preserves comments and the
comment is the marker. A block scalar is one event, so text inside it is never mistaken for a key.

### Commands

The producer side reads a document from a path or stdin and writes to stdout:

| Command | Does |
|---|---|
| `render --format <f>` | Markdown tables, a commented `config.toml`, a JSON Schema, labels, a Dockerfile block |
| `stamp` | writes build identity onto a document |
| `conform --tier <n>` | runs the refusals and a conformance tier |
| `validate` | checks a document against `spec/v1/contract.schema.json` |
| `image verify` / `image attach` | holds an image to the document, and attaches it as a referrer |

The consumer side runs in a chart repository: `pull` vendors contracts, `check` holds a render to
them, `bindings`, `shapes`, `coverage`, `readme`, `reload` and `tests` write or check what the chart
derives from them, `diff`, `migrate` and `secrets` report, `explain` answers questions, `adopt` and
`helm init` write values, and `sync` runs the writers or, with `--check`, every offline gate in
order. The per-command guides are under [`cli/docs/`](../cli/docs/).

Every writer and its checker are one subcommand, with `--check`, so the two cannot disagree about
what "already correct" means. The writers and checkers exit `0` clean, `1` findings and `2` could
not read. `diff --exit-code`, `migrate`, `explain` and `secrets` report rather than check, and
each one's status contract is in its guide.

The binary does not produce a contract, which needs the types; does not write `Chart.yaml`, because
only a reviewer knows whether a moved key matters; does not run `helm template`, because the gates
must read the same render every other linter reads; and reaches the network only in `pull`,
`sync` (through its `pull` step, unless `--no-pull`), `helm init`, `image verify --image` and
`image attach`.

### Refusals: normative, with a fast path

`conform` is normative and runs in every build as the gate. A per-language validator may also run
inside the producer, because it knows the field, annotation and source position where the binary
knows only a JSON pointer. Its only job is a better message: where the two disagree, `conform` is
right and the other is a bug.

## Renderings are artefacts of the spec

Every rendering is a pure function of the document and its rendering options, so the corpus
carries the rendered bytes for one fixed set of options: the default columns, the default JSON
Schema and TOML example settings, and the default in-image path for the labels and the Dockerfile
block:

```text
spec/v1/conformance/<case>/
  contract.json
  README.md
  rendered/
    markdown.md  markdown-loader.md  markdown-keys.md  config.toml
    schema.json  labels.txt          Dockerfile.part
```

The goldens have one author, the Rust reference implementation's `TERRACE_SPEC_BLESS` run (the
command is in `spec/v1/CONFORMANCE.md`). `terrace-contract` renders the same documents and is
checked against those bytes, never blessing them, as is anything else that renders, so a drift is a
failing test in the implementation that drifted. Rendering is shared, so two documents that are
equal render to equal bytes whichever language produced them. The documents themselves are equal
only between implementations sharing a loader and a type mapping, which is why `CONFORMANCE.md`
gives Java its own goldens: its type names and JSON Schema rendering differ from Rust's.

## Reading documents this repository did not write

Three implementations produce documents, through three different loaders, so the consumer reads
nothing by assumption.

- **Value reads belong to the loader.** `value` is a registry keyed by `producer.loader`. A loader the
  registry does not know has its value check skipped and reported as skipped, never performed by
  another loader's rules, because a pattern refusing text the real loader accepts stops a correct
  deployment. Each implementation carries a test asserting its loader agrees with its row.
- **Derive nothing the document states.** Rules that read `env`, `env_file`, `secrets_file` and
  `unreachable` as published are tier 1 work and run unconditionally. Only rules that must derive a
  spelling depend on tier 2, so a Spring producer, whose relaxed binding cannot reach tier 2, is
  usable by every other gate.
- **`ty` is not a type name.** No rule switches on it; `constraint`, `text_form` and
  `text_constraint` carry everything a rule needs.
- **`schema_version: 1` documents still exist.** Element schemas degrade to "none published" and the
  generators narrow rather than refuse.

The corpus holds consumer cases for each: `foreign-loader`, `foreign-ty`, `schema-version-1` and
`tier-1-spellings`.

## The chart repository side

### The declaration

`charts/<chart>/config-contract.yaml` says which rendered objects hold configuration, which images
read each one, which workloads mount it, and which values files are exempt from which gate, each
with a reason. Adoption is opt-in: `coverage` warns about a chart that pins a first-party image and
declares no documents, fails a declaration that leaves one of the chart's own first-party images
unconfigured, and stops with an error on a declaration it cannot read. An empty `documents: []` is
an explicit opt-out and must carry a `reason`. The format, the `@config` marker language and the
repository settings are specified in [`spec/helm/v1/`](../spec/helm/v1/), versioned apart from
`spec/v1/`, whose `$id` promises it will describe `terrace_contract: 1` and nothing else.

### Vendored contracts

`charts/<chart>/contracts/<component>.json` is committed and refreshed by `pull`, which verifies the
signature and the labels before writing. Vendoring keeps every gate offline, keeps an old commit
reproducible against what was true then, and, most importantly, puts the contract diff in the same
pull request as the digest bump, where a reviewer sees the removed key beside the gate it fails.

### Unioning contracts

One rendered document can be read by several binaries, each of which describes only the keys it
consumes. Validating against one binary's schema with closed objects would reject every key the
others read, so the gates validate against the union of every reading image's `json_schema`:

| Situation | Rule |
|---|---|
| a path in one schema only | kept |
| a path in several, identical | kept once |
| `additionalProperties` | `false` at every level whose properties are enumerated: after the union, an unknown key is unknown to all. A map keeps its element schema |
| `title`, `description` | the first is kept: prose differs by construction |
| any other keyword with different values | a hard error: two binaries disagree about one key |

`default`, `$schema` and `$id` are assertions here: two binaries with different fallbacks for one
key make the value an operator gets depend on which binary read the document.

## The gates

Gates 1 to 3 are offline, share one render with every other linter, and run on every pull request.

**Gate 1, the document.** For every chart and every CI values file: select the object by kind and
selector, parse the document, and validate it against the union with closed objects. It catches an
unknown key, a wrong type, a value outside an enum, a property the schema's own `required` lists,
and a table where a scalar belongs. It does not check the contract key's `required` flag, which says
some layer must supply the key rather than that the document carries it: a secret supplied by a
mounted file is correct and absent from the document.

**Gate 2, the environment.** Per container, against that container's own image's contract, because a
variable only a sibling image reads is the defect this gate exists for. Every variable is classified
by an ordered list, first match wins, and the step rejecting an unaccounted prefixed variable sits
above both external lists. The variables include every key an `envFrom` source hands over, followed
to the `ConfigMap` or Secret the render holds; a Secret's values are never read, and a source naming
an object the render does not hold is recorded rather than guessed at. A value the manifest carries
is checked against `text_constraint`. When the build has a measured read table for
`producer.loader`, the value is then parsed as `text_form` says and checked against `constraint`;
for any other loader that step is reported as skipped, never as passed. A `valueFrom` entry is
classified by its name, but its value exists only when the pod runs, so it is not checked. The gate
also finds a key supplied by two layers at once, aliases included, which the loader refuses at boot,
and names both spellings. When an unaccounted variable looks like a service link and the pod does
not set `enableServiceLinks: false`, the message says so.

**Gate 2b, requiredness.** Per container, against the keys that container's image marks
`required`. Each must be supplied by some layer: an environment spelling (`env`, `valueFrom` and
`envFrom` all count), a `_FILE` variable, a file in the secrets directory, or the rendered document.
A secret is never supplied by the document, a reserved key only by the environment, and a key a
file cannot supply (one whose `text_form` is not `text`) by neither file layer. The finding
names every layer that could supply the key, which tells an operator there is a choice where a
`required` list would tell them to put a credential in a ConfigMap. A source the render does not
show (an `envFrom` object or a secrets-directory Secret it does not hold, a volume of another type,
a secrets directory whose path is itself a Secret value)
could carry the key, so the finding is a warning naming it, unless the source is a file layer and
the key one no file supplies. The gate does not run for a values file
exempt from gate 2 or 3, and treats non-secret keys as undecided when the document cannot be read,
because in both cases a layer it did not read may supply the key.

**Gate 3, file spellings.** Per container. File names are read from the `Secret` and `ConfigMap`
sources of each mounted volume, as far as the render shows them; other volume types present nothing
the gate can enumerate. Every such file at the secrets directory must spell some key's
`secrets_file` or one of its aliases, or address an element of a dynamic map. One mounted elsewhere
that spells a key must be named by a `_FILE` variable, or nothing reads it. A variable gate 2
classified as a key's `env_file` must, when the manifest carries its value, point inside a mounted
volume; a `valueFrom` path exists only at run time and is not checked. Any other `*_FILE` variable
is gate 2's to account for. The key a file supplies must have `text_form: text` (stricter than the
`constraint` rule above), because a file's contents reach the loader as a string with no parse.

**Gate 4, the real loader.** The service binary loads the rendered configuration with its real
types, defaults and shadow policy, and exits non-zero with the loader's own message. It needs an
image pull per component, so it runs beside end-to-end tests rather than on every pull request.
Gates 1 to 3 are the fast approximation and gate 4 is the ground truth; having both is what lets the
first three stay cheap.

## Distribution and versioning

`terrace-contract` ships as a multi-architecture image at `ghcr.io/timschoenle/terrace-contract`
and as four release binaries (`x86_64` and `aarch64` Linux on musl, Windows, macOS on Apple silicon)
with a signed `SHA256SUMS`. The image is built from `scratch` and holds only the binary, so a
consumer's Docker build copies the binary out of it into a stage that has a shell to run it in. The
Gradle plugin runs the same binary from a Java build, and `TERRACE_CONTRACT_BIN` points any caller
at a local build.

`cli/` is its own release-please package and versions independently of every implementation. Its
compatibility statement is the envelope versions it reads, `terrace_contract: 1` and
`schema.schema_version` up to the current one, never a producer name or version.

## Testing

- **The corpus** tests the renderings: one set of bytes, regenerated by one command, verified by
  every implementation that renders.
- **The chart rules** are integration tests that build a document and a manifest, call the rule and
  read the findings back.
- **The parity harness** (`cli/tests/parity.rs`) runs every chart rule over a real chart repository
  at a pinned ref, including a deliberately broken copy of its render, because a clean corpus proves
  only that nothing crashed.
- **Fuzz oracles** in `cli/fuzz/` and `rust/fuzz/` replay a committed corpus and a fixed-seed sweep
  under a plain `cargo test`.

## Designed for, not built

- **Cluster-side admission.** A policy engine fetches the same referrer for a running pod's digest
  and refuses a mismatched pair; the in-image copy is what makes this work without a registry.
- **Non-TOML documents.** `source.format` is already a field, and a YAML or JSON document normalises
  to the same tree.
- **Hand-written contracts for third-party images.** Every gate runs identically; only the source of
  the document differs.
- **The image's own `ENV` block.** A Dockerfile `ENV` supplies a key on every run, which the
  contract's `default` cannot know; reading it is one `crane config` away.
- **Secret values, not only names.** A rendered Secret carries its values, so gate 3 could hold them
  to `constraint` with the file layers' one read rule.
- **A drift report.** The same union across every published chart version and image digest answers
  which releases run on configuration their image no longer reads.

## Risks, and what answers each

| Risk | Answer |
|---|---|
| A registry outage fails every pull request | contracts are vendored, so the pull-request gates read the committed copies and run offline |
| A swapped contract weakens the gate | signature-verified at refresh, fetched by digest, committed and reviewed |
| A remote `$ref` makes CI depend on a third party | the validator refuses any `$ref` that is not `#/…` |
| The schema is more permissive than the loader | bounded by design; gate 4 is the answer, not tighter guessing |
| The schema is stricter than the loader | the rule throughout: a schema that rejects what the loader accepts is worse than one that accepts what it rejects |
| An escape hatch cannot be validated | exempted per values file and gate, with a reason |
| A chart and an image release out of order | everything keys off the pinned digest, so the failure lands on the bump |
| A new chart quietly opts out | `coverage` warns about a first-party image with no declaration; opting out takes `documents: []` and a `reason` |
| A rule quietly assumes one loader again | the loader registry returns `Option`, so a skip cannot be forgotten |
| `cli/` grows a dependency on an implementation | a `cargo tree` assertion in CI |
