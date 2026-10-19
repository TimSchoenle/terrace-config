# terrace-contract

Everything the configuration contract needs *after* a document exists.

A service publishes one JSON document describing every setting its binary reads —
[`spec/v1/`](../spec/README.md) is what that document is. Producing one needs the service's own
types and can only be per-language. Everything downstream of it needs a document and nothing else,
and that is this binary.

```text
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

`image verify --dockerfile` compares every `# terrace-config:labels:begin`/`:end` region in the file,
because a Dockerfile building several images carries one region per stage. An empty region is a
finding on its line; a file with no region, an unterminated or nested one, or more than 32 is
refused.

Every command reads a document from a path or from `-`, so the producing step and the rendering
step compose without a temporary file between them:

```bash
./gradlew -q :terrace-config-example-service:run --args=--contract | terrace-contract conform --tier 1 -
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
FROM ghcr.io/timschoenle/terrace-contract:0.7 AS contract
COPY --from=contract-builder /out/contract.json /in/contract.json
RUN terrace-contract conform --tier 2 /in/contract.json \
 && terrace-contract render --format labels /in/contract.json > /out/labels
```

Pin the minor while the version is below `1.0`: a minor release may break, so `0.7` takes every fix
and nothing else, where `0` would take the breaking releases too.

A Spring Boot service's Dockerfile is that same file with a different `contract-builder` stage.

## Charts whose images reload

An image may publish that it applies a change without restarting (`schema.reload`), and which keys
only a restart applies (`keys[].reload`). A chart that hashes its whole configuration into a pod
annotation throws that reload away; one that hashes none of it leaves every restart-only key changed
on disk and never applied. `reload` writes the part each chart should hash:

```bash
terrace-contract reload --charts charts          # writes charts/<chart>/templates/_config-reload.tpl
terrace-contract reload --check --charts charts  # CI: fails when a file is behind its contracts
```

`check` then holds each render to it: a long-running workload reading a document with restart-only
keys must carry a `checksum/` pod annotation, and no key the image applies live may arrive through
a `subPath` mount or an immutable object, which the kubelet never updates. A chart may add keys to
the restart set with a `restart:` entry per document in `config-contract.yaml`, each with a reason,
and never remove one. `explain` prints what a change to each setting costs.

Nothing changes for a chart until one of its images declares a rebuild. The rules are in
[`FORMAT.md`](../spec/v1/FORMAT.md#reloading).

## Repository settings: `terrace.yaml`

A chart repository keeps its settings in `terrace.yaml` at the root of its Git repository, instead
of repeating flags in every script:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/TimSchoenle/terrace-config/main/spec/helm/v1/repo.schema.json
version: 1
cli: "0.8"
helm:
  charts: charts
  firstParty: [ghcr.io/example/*]
  signer: https://github.com/example/[^/]+/.github/workflows/release.yaml@refs/heads/main
```

```bash
terrace-contract config init   # writes one, moving .github/configs/first-party-images.txt into it
terrace-contract config show   # every setting, its value, and where it came from
terrace-contract doctor        # the settings, the pinned CLI and the tools, one line each
```

A flag wins over its environment variable, which wins over the file, which wins over the default.
The file is optional, and a repository without one behaves as before. A key this release does not
read is a warning, and fails the command under `--strict` or `TERRACE_STRICT=1`. Every key, its
flag and its default are in [`spec/helm/v1/REPO.md`](../spec/helm/v1/REPO.md).

## Checking only the charts a change touched

By default, `check`, `bindings`, `shapes`, `readme` and `tests` read every chart under `--charts`.
Each of them also takes chart names after its options, and then reads only those charts:

```bash
terrace-contract bindings --format github portfolio netcup-offer-bot
terrace-contract tests --check portfolio
```

A name that matches no chart directory exits `2`. A misspelt name would otherwise select nothing,
and a gate that checks nothing passes.

`scope` works out the names from Git. It prints the application charts that the commits since a
revision touched, measured from the merge base with `HEAD`, on one line:

```bash
terrace-contract scope --since origin/main          # e.g. "portfolio tankovault"
terrace-contract scope --since origin/main --json   # the same, with the reason it widened
```

The line is empty, meaning every chart, whenever narrowing could miss a chart the change breaks:
a changed path outside the charts' own directories (the pinned CLI, `terrace.yaml`, a workflow), a
change to a chart that is a library at the base or at `HEAD` (deleting a library, or turning one
into an application chart, still widens), or a change that cannot be read, such as in a shallow
clone. Why it
widened goes to stderr. It can therefore widen a gate and never narrow one by mistake.

## Adopting keys a new contract adds: `adopt`

When an image release adds a setting, `bindings` fails because no chart value reaches it. `adopt`
writes the repair for the cases that need no judgement:

- an ordinary key gets a chart value, with its type, default and description taken from the
  contract, and the marker that binds it;
- a credential gets a value delivered as a file, with no marker, plus its `unbound` write-off and
  its `credentials` row;
- a reserved key gets its write-off and no value.

The template lines that project a new value are appended to `derivedConfig` and `secretData`.
That only happens where the define does not already write the key's root, and, in a
multi-document chart, only where the `$service` guard can be read from the declaration. Anywhere
else the lines are printed for a person to place.

```bash
terrace-contract adopt                                # plan every enrolled chart and print it
terrace-contract adopt portfolio --key sentry.debug   # one chart, only the named keys
terrace-contract adopt --check                        # exit 1 when any enrolled chart owes a key
terrace-contract adopt --diff                         # the unified diff --write would apply
terrace-contract adopt --write                        # apply it
```

`adopt` never touches a value that already exists. A key whose chart value is already there is
refused, and the refusal names the marker to write by hand. Only a person can say which key an
existing value feeds. A chart whose markers are broken is blocked and nothing is planned for it.

Every edit inserts whole lines and changes no existing byte: each block goes after the last bound
sibling of the value it belongs with, CRLF files stay CRLF, and YAML is never re-serialised.
`--diff` writes nothing. Its output applies with `patch -p0` from the directory `adopt` ran in, and
it lists blocked charts and refused keys on stderr.

Exit codes:

- Without `--check`: `1` when a key was refused, a chart is blocked, or the run failed; `0`
  otherwise.
- With `--check`: `1` when anything is owed; `2` when the run could not answer.
- Both: `2` for a chart name that is not a chart, or `--key` without a chart.

`adopt` is not part of `sync`. Writing a setting into a chart is a change somebody should read.

## One command for CI: `sync`

`sync` runs the steps a chart repository needs in the right order, so a pipeline or a person runs
one command instead of a list of them.

```bash
terrace-contract sync                                # pull, then rewrite shapes and README references
terrace-contract sync --no-pull                      # the same from the contracts already vendored
terrace-contract sync --check --since origin/main    # every offline gate, on the charts the change touched
terrace-contract sync --check --manifests rendered   # and `check` on a render, too
```

Without `--check` it writes: `pull`, then `shapes`, then `readme`, each reading what the one before
it wrote. It does not write the round-trip suites, because a generated test that lands without its
author reading it is a test nobody has read; run `tests` for those.

With `--check` it writes nothing and needs no network. It runs `bindings`, `shapes --check`,
`readme --check`, `tests --check` and, when `helm.firstParty` is set, `coverage`. With `--manifests`
it also runs `check` on that render. Every step runs even when an earlier one failed, so one run
reports everything. The exit code is `0` when every step passed, `1` when a step found something,
and `2` when a step could not run. `--format github` puts each step in a collapsible log group and
writes the findings into the job summary.

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

Its own Cargo workspace, and a sibling of [`../rust`](../rust) rather than a member of it — the
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
