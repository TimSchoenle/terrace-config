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
terrace-contract render --format dockerfile --write Dockerfile contract.json
terrace-contract image verify --image ghcr.io/you/api:v2.5.0 --dockerfile Dockerfile contract.json
terrace-contract image attach --image ghcr.io/you/api:v2.5.0 contract.json
```

### The label block

`render --format dockerfile --write Dockerfile` rewrites every `# terrace-config:labels:begin`/`:end`
region in the file in place, and `--check Dockerfile` exits 1 when any region is not the block the
document renders. Write the two marker lines where the labels belong once; `--write` fills them, and
leaves everything outside the regions byte for byte. `image verify --dockerfile` is the same
comparison. A Dockerfile building several images carries one region per stage, and every region is
compared or rewritten. An empty region is a finding on its line; a file with no region, an
unterminated or nested one, or more than 32 is refused.

### The built image

`image verify --image REF` reads the image itself through `oras`, so no `docker inspect` output has
to be captured first:

- the contract labels of every platform, which must agree with each other and with the document;
- the file at the labelled path inside the image, which must be the document byte for byte;
- the referrer attached to the digest, if there is one, which must be the document byte for byte;
- at least one of those two carriers, since an image publishing neither publishes no contract.

`REF` is fully qualified (`docker.io/library/redis:7`, not `redis:7`); a tag is resolved once and
every check is made against the digest. `--oci-layout` reads `REF` as an OCI image layout — a
directory or tarball, as `docker buildx build --output type=oci` writes — so the image is checked
after it is built and before it is pushed. `--signer IDENTITY` also verifies the keyless `cosign`
signature against that identity under the GitHub Actions issuer. `--labels FILE` remains the
offline form of the label check, for what `docker inspect` printed. Exit 1 means the image is wrong,
2 that it could not be read.

`image attach --image REF contract.json` attaches the document to the digest as a referrer of
artifact type `application/vnd.terrace.config-schema.v1+json` and prints the referrer's digest, which
is what `cosign sign` then signs. It is safe to re-run: a referrer already holding the document byte
for byte is reported rather than attached twice, and one holding a different document is refused.
With `--oci-layout` the layout must be a directory, since the referrer is written into it.

Every command reads a document from a path or from `-`, so the producing step and the rendering
step compose without a temporary file between them:

```bash
java -cp "$RUNTIME_CLASSPATH" de.timscho.config.loader.ContractMain \
    --type com.example.Config --prefix MYAPP_ --app-name my-service \
  | terrace-contract stamp --app-version v2.5.0 - \
  | terrace-contract conform --tier 2 -
```

The Java side's `de.timscho.terrace-config` Gradle plugin runs the same three steps, through files
rather than a pipe, as its `terraceContractEmit` and `terraceContract` tasks (see
[`java/README.md`](../java/README.md#terrace-config-gradle-plugin)).

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
terrace-contract config init                    # writes one, moving .github/configs/first-party-images.txt into it
terrace-contract config show                    # every setting, its value, and where it came from
terrace-contract config show --chart portfolio  # the chart's declaration with every default written out
terrace-contract doctor                         # the settings, the pinned CLI and the tools, one line each
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
`--write` stages every file of every chart before it replaces any of them, and puts back what it
replaced if a later replacement fails, so a run that fails part-way leaves the tree as it was. It refuses to write a file that resolves outside its chart.
`--diff` writes nothing. Its output applies with `patch -p0` from the directory `adopt` ran in, and
it lists blocked charts and refused keys on stderr.

Exit codes:

- Without `--check`: `1` when a key was refused, a chart is blocked, or the run failed; `0`
  otherwise.
- With `--check`: `1` when anything is owed; `2` when the run could not answer.
- Both: `2` for a chart name that is not a chart, or `--key` without a chart.

`adopt` is not part of `sync`. Writing a setting into a chart is a change somebody should read.

## Starting a chart from its image's contract: `helm init`

A chart has two halves. The chassis is the same in every chart of a repository: `Chart.yaml`, the
workload templates and the README source. The repository keeps it as a template and copies it. The
configuration half comes from the image's contract, and `helm init` writes it:

| file | what it is |
|---|---|
| `contracts/<document>.json` | the image's contract, fetched and verified exactly as `pull` does it |
| `values.yaml` | the image pin, a block per setting with the marker that binds it, then `--chassis` |
| `templates/_helpers.tpl` | the partials that project the values into the document and the Secret |
| `config-contract.yaml` | the declaration: only what differs from the [defaults](../spec/helm/v1/DEFAULTS.md), `bindings: true`, `tests: true` with a prerequisite per required credential, and a reasoned `unbound` entry per write-off |
| `ci/test-values.yaml` | the credentials the render guard demands, as values that can never authenticate |
| `tests/configmap_test.yaml` | a first suite for a person to extend |

```bash
terrace-contract helm init portfolio --image timschoenle/portfolio:v3.1.3@sha256:...           # print the plan
terrace-contract helm init portfolio --image timschoenle/portfolio:v3.1.3@sha256:... --diff    # the diff --write would apply
terrace-contract helm init portfolio --image timschoenle/portfolio:v3.1.3@sha256:... --write \
  --chassis .github/templates/chart/values.chassis.yaml
terrace-contract helm init redis --image library/redis:8.0 --no-contract --write              # no contract: the plain form
```

`--document` names the document in the declaration and defaults to the chart's name.
`--document-key` is the `ConfigMap` key it is rendered under and defaults to `config.toml`. The
signer comes from `--signer`, `CONTRACT_SIGNER` or `helm.signer`, as for `pull`. There is no way to
hand `init` a contract file: a contract that skipped verification would be trusted by every gate
downstream.

**The placeholder protocol.** The copied templates have to name things only the contract knows:
the environment prefix, the nesting separator and the document key. They spell them
`%%PREFIX%%`, `%%ENV_PREFIX%%` (the prefix without its trailing separator), `%%SEPARATOR%%` and
`%%DOCUMENT_KEY%%`, a form Helm never reads. `init` fills them in every file of the chart. It skips
`charts/` and `contracts/` at the top level, and any file that is not UTF-8. Any other `%%NAME%%`
still present is refused, because it is the copy's to substitute and it would otherwise render as
literal text. So the copy can be a `sed` over the chart's name, description and versions, and
nothing in it has to read JSON.

**The plain form.** `--no-contract` writes `values.yaml` with only the image and the chassis, an
empty `_helpers.tpl` and an empty fixture. It writes no `ConfigMap`, no Secret and no
`configMount`, because each describes a loader nothing knows this image has. A first-party image,
by the same list `coverage` reads, still owes a declaration. `--reason` is required in that case,
and is written as the declaration's opt-out. A third-party image gets no declaration. The
placeholders are filled from the chart's name (`REDIS_`, `__`), since there is no dialect to read.

**It replaces nothing.** Every file is planned before one is written. A file already there that
says something else is refused, and the error names each one. A file that already holds exactly
what would be written is left alone, so a second run owes nothing. The vendored contract counts as
unchanged when only its fetch time moved. A chart that already has a configuration is `adopt`'s
to change.

After `--write`, it prints what is left for a person, in order:

- the grouping blocks whose description is a `TODO`, because a contract describes keys and not the
  blocks a chart groups them into;
- the values it had to invent, for keys the image requires and publishes no default for;
- the `unbound` reasons to read.

Exit codes:

- `--check`: `1` when anything would be written; `0` when the chart already holds exactly this;
  `2` when it could not answer.
- `--diff`: prints a `patch -p0` diff and exits `0`.
- Otherwise: `1` for any failure.

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

## Markers a template never renders

`bindings` holds every `# @config` marker in an enrolled chart (`bindings: true` in
`config-contract.yaml`) against the contract it names. It also checks that the chart's templates
render each bound key at all: for every `projection`, `structured` and `composed` marker, the last
segment of the contract key must appear in some `templates/*.tpl` file, either as a mapping key
(`interval_secs:`) or as a quoted `dict`/`set` argument (`"interval_secs"`). Otherwise a value can be
bound, typed and documented while the template renders nothing for it.

The check is a plain text search. A hit means the key's name is rendered somewhere, and does not mean
it is wired to the right value. Because it is part of `bindings`, `sync --check` runs it with no
extra step.

## After a contract removes something: `migrate`

A refresh writes values for the keys an image added. It cannot follow a removal, because the lines
still using what went away are in templates, hand-written test suites and CI values files, which no
writer edits. `migrate` lists those lines:

```bash
terrace-contract migrate tankovault --since "$(git merge-base HEAD origin/main)"
terrace-contract migrate --json                     # every chart, against origin/main
```

```text
==> hand migration required: the contract took these away, and no generator edits
    the templates, hand-written tests or CI values that still use them
  tankovault: legal.dir: key removed (chart value legal.dir)
      charts/tankovault/templates/_config.tpl:515: dir: {{ $ctx.Values.legal.dir | quote }}
  tankovault: legal.documents: element property 'sources' removed (chart value legal.documents)
      charts/tankovault/tests/legal_test.yaml:58: legal.documents.privacy.sources.en: /mnt/legal/privacy.en.md
```

It reports two kinds of change: a contract key that was removed, and a property removed from the
schema of a key's elements. It compares the vendored contracts with the revision given in `--since`,
in the same way as `diff`. It reads the chart's markers from `values.yaml` **as it was at that
revision**, because by the time anyone looks, the value a removed key was bound to has usually been
deleted. It searches `templates/`, `tests/*.yaml` and `ci/*.yaml`, and skips the generated
`contract_roundtrip_*_test.yaml` suites, because running `tests` regenerates those.

Pass the commit the branch started from, not `HEAD`. On a bump branch the refreshed contract is
usually committed already, so nothing has changed since `HEAD`.

The search is plain text, so a hit is a place to look rather than proof that the line is wrong.
Every removal is listed, including ones nothing references, so you can confirm that a migration is
complete. The exit code is `0` when no line still uses a removal, `2` when one does, and `1` when the
command could not answer (an unknown revision, a chart name that matches no chart, or a contract or
marker it cannot read). These are the same three outcomes as `diff --exit-code`.

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
`stamp` and `image`. `--features oci` adds the registry reads and the attach, still without any chart
concept. Keeping it compiling is the mechanical check that no consumer-side concept —
a chart, a manifest, a Helm marker — has leaked down into it.

See [`docs/contract-cli-plan.md`](../docs/contract-cli-plan.md) for what is built and what is not.
