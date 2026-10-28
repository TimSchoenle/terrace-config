<!--
Generated from .github/templates/cli-README.md.hbs. Edit that file, not this one.

CI renders it on every pull request and commits the result back to the branch. A push to `main`
whose cli/README.md does not match its template fails the `readme` job in
.github/workflows/docs.yml.

The payload comes from TimSchoenle/actions/actions/common/readme-variables, which reads
cli/Cargo.toml and walks cli/docs/, merged over the output of one command:

    bash .github/scripts/readme-variables.sh cli/Cargo.toml

The release tag, the image tag, the MSRV and the documentation table all come from that payload,
so the release pull request is the commit that corrects them.

Nothing in this comment may contain a mustache that is not a real reference.
-->

# terrace-contract

Read, render, check and gate configuration contracts, in any producer's language.

[![Release](https://img.shields.io/github/v/release/TimSchoenle/terrace-config?sort=semver&filter=terrace-contract-v*)](https://github.com/TimSchoenle/terrace-config/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/TimSchoenle/terrace-config/ci.yml?branch=main&label=ci)](https://github.com/TimSchoenle/terrace-config/actions/workflows/ci.yml)
[![Coverage](https://codecov.io/gh/TimSchoenle/terrace-config/graph/badge.svg)](https://codecov.io/gh/TimSchoenle/terrace-config)
[![Licence](https://img.shields.io/github/license/TimSchoenle/terrace-config)](../LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](Cargo.toml)

## What this is

A service publishes one JSON document describing every setting its binary reads, and
[`spec/v1/`](../spec/README.md) defines that document. Producing one needs the service's own types,
so it is per language. Everything downstream of it needs only the document, and that is this
binary.

```text
per language, once                    this binary, for everyone
───────────────────────────────       ─────────────────────────────────────────
types ──> Schema ──> Contract ──JSON──> render    the tables, the file, the labels
                                        stamp     build identity onto a document
                                        conform   the refusals, and a tier
                                        validate  the published meta-schema
                                        image     labels and Dockerfile block, read back
```

**A new implementation's obligation is one sentence: emit a document that passes
`terrace-contract conform`.** It writes no renderer per format, no validator and no test kit.
Every rendering is a pure function of the published document, so it runs for every producer,
including ones nobody has written yet. `spec/v1/conformance/<case>/rendered/` holds that to account:
the Rust implementation blesses those bytes from its types, this binary renders the same bytes from
the document alone, and [`tests/corpus.rs`](tests/corpus.rs) fails if the two disagree.

## Quick start

```bash
docker run --rm -v "$PWD:/work" -w /work \
  ghcr.io/timschoenle/terrace-contract:0.12 conform --tier 2 contract.json
```

## Table of contents

- [Features](#features)
- [Installation](#installation)
- [Usage](#usage)
- [Compatibility](#compatibility)
- [Documentation](#documentation)
- [Contributing](#contributing)

## Features

- **Render** a document as Markdown tables, a commented `config.toml`, a JSON Schema, image labels
  or a Dockerfile `LABEL` block.
- **Gate** it: `conform` runs the refusals and a conformance tier, `validate` the meta-schema, and
  `stamp` writes build identity onto it.
- **Check images**: `image verify` holds a Dockerfile, a built image or an OCI layout to the
  document, and `image attach` attaches it as a signed referrer. See [IMAGES.md](docs/IMAGES.md).
- **Gate Helm charts** against the contracts of the images they pin: `check`, `bindings`, `shapes`,
  `diff`, `coverage`, `secrets` and `pull`, plus `sync` to run them in order, `adopt` and
  `helm init` to write chart values from a contract, and `migrate` to find what a removal broke.

It deliberately does not:

- **Produce a contract.** That needs the types. It is the seam.
- **Depend on any implementation**, not on `terrace-config` and not transitively. A renderer
  reaching for one producer's types would be that producer's renderer under a shared name, so CI
  asserts the absence.
- **Guess at a loader it does not know.** `producer.loader` names the library whose environment
  reads a document's `text_constraint` patterns were measured against. A consumer meeting an
  unknown one skips the read that depends on them and says so, rather than performing it with the
  wrong rules.

## Installation

The image is multi-architecture and signed keylessly:

```dockerfile
FROM ghcr.io/timschoenle/terrace-contract:0.12 AS contract
```

The tag is the release series. Below `1.0` a minor release may break, so the series is
`major.minor` and takes every fix and nothing else; from `1.0` it is the major alone.

Each release also attaches four binaries, named
`terrace-contract-0.12.0-<target>.tar.gz` (`.zip` on Windows), with `SHA256SUMS`
and its signature beside them:

```bash
curl -fsSLO https://github.com/TimSchoenle/terrace-config/releases/download/terrace-contract-v0.12.0/terrace-contract-0.12.0-x86_64-unknown-linux-musl.tar.gz
curl -fsSLO https://github.com/TimSchoenle/terrace-config/releases/download/terrace-contract-v0.12.0/SHA256SUMS
curl -fsSLO https://github.com/TimSchoenle/terrace-config/releases/download/terrace-contract-v0.12.0/SHA256SUMS.sigstore.json
cosign verify-blob SHA256SUMS --bundle SHA256SUMS.sigstore.json \
  --certificate-identity 'https://github.com/TimSchoenle/terrace-config/.github/workflows/release-cli.yml@refs/heads/main' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum --check --ignore-missing SHA256SUMS
```

The targets are `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-pc-windows-msvc`
and `aarch64-apple-darwin`. The crate is not published to crates.io.

## Usage

```bash
terrace-contract render --format markdown contract.json >> README.md
terrace-contract render --format toml contract.json > config.example.toml
terrace-contract render --format json-schema --title "portfolio configuration" contract.json

terrace-contract conform --tier 2 contract.json
terrace-contract validate contract.json

terrace-contract stamp --app-version "v2.5.0" --revision "$GIT_SHA" contract.json
terrace-contract render --format dockerfile --write Dockerfile contract.json
terrace-contract image verify --image ghcr.io/you/api:v2.5.0 --dockerfile Dockerfile contract.json
terrace-contract image attach --image ghcr.io/you/api:v2.5.0 contract.json
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

Every command reads a document from a path or from `-`, so the producing step and the rendering
step compose without a temporary file between them:

```bash
java -cp "$RUNTIME_CLASSPATH" de.timscho.config.loader.ContractMain \
    --type com.example.Config --prefix MYAPP_ --app-name my-service \
  | terrace-contract stamp --app-version v2.5.0 - \
  | terrace-contract conform --tier 2 -
```

The Java side's `de.timscho.terrace-config` Gradle plugin runs the same three steps through files,
as its `terraceContractEmit` and `terraceContract` tasks (see
[`java/README.md`](../java/README.md#the-gradle-plugin)).

Exit `0` clean, `1` read it and found it wanting, `2` could not read it. A pipeline that conflates
the last two cannot tell a failing gate from a broken one.

In a Docker build, the contract-producing stage is the only language-specific part, and a Spring
Boot service's Dockerfile differs only in its `contract-builder` stage:

```dockerfile
FROM ghcr.io/timschoenle/terrace-contract:0.12 AS contract
COPY --from=contract-builder /out/contract.json /in/contract.json
RUN terrace-contract conform --tier 2 /in/contract.json \
 && terrace-contract render --format labels /in/contract.json > /out/labels
```

A chart repository configures the chart commands in `terrace.yaml` and runs them through `sync`;
see [REPOSITORY.md](docs/REPOSITORY.md).

## Compatibility

- Rust 1.94 or later builds it from source.
- `--no-default-features` leaves the producer-side half: the document model, the renderers,
  `conform`, `stamp` and `image`. `--features oci` adds the registry reads and the attach, still
  without any chart concept.
- `terrace-contract --version` prints which `terrace_contract` and `schema_version` values the
  release reads; [`spec/v1/FORMAT.md`](../spec/v1/FORMAT.md) defines both.

## Documentation

| Document | Summary |
|---|---|
| [Adopting keys a new contract adds: adopt](../cli/docs/ADOPT.md) | How adopt writes the chart values, write-offs and template lines for keys an image release adds, and which cases it leaves to a person. |
| [Starting a chart from its image's contract: helm init](../cli/docs/HELM-INIT.md) | How helm init writes the configuration half of a new chart from its image's contract, and the placeholder protocol that lets a repository copy its own chassis around it. |
| [Images](../cli/docs/IMAGES.md) | How terrace-contract writes the label block into a Dockerfile and checks a built image against its contract: the labels, the embedded file and the attached referrer. |
| [After a contract removes something: migrate](../cli/docs/MIGRATE.md) | How migrate finds the template, test and CI lines that still use a key or element property a refreshed contract removed. |
| [Charts whose images reload](../cli/docs/RELOAD.md) | How a chart decides which configuration changes restart its pods, from what each image's contract says it applies without a restart. |
| [Chart repository settings and gates](../cli/docs/REPOSITORY.md) | How a chart repository configures terrace-contract in terrace.yaml, narrows its gates to the charts a change touched, runs every step with sync, and checks that templates render what their markers… |

The design and what is built are in [`docs/contract-cli-plan.md`](../docs/contract-cli-plan.md).

## Contributing

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org).

`cli/README.md` is generated. Edit `.github/templates/cli-README.md.hbs` instead.

The binary is its own Cargo workspace, a sibling of [`../rust`](../rust) rather than a member of
it: it is not part of the Rust implementation, it is written in Rust.

```bash
cd cli
cargo test
cargo clippy --all-targets --no-default-features   # the half a language's build links
```

Keeping `--no-default-features` compiling is the mechanical check that no consumer-side concept,
such as a chart, a manifest or a Helm marker, has leaked into the producer-side half.

## Security

[SECURITY.md](../SECURITY.md) has the reporting instructions. Do not open a public issue for a
vulnerability.

## Licence

`MIT`. See [LICENSE](../LICENSE) for the terms.
