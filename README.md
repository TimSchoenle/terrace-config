<!--
Generated from .github/templates/root-README.md.hbs — edit that file, not this one.

CI renders it on every pull request and commits the result back to the branch, the same way it
does for rust/README.md and java/README.md. See .github/workflows/docs.yml.

This is the repository's front door: what the project is, its features, one table of
implementations, and a link into each implementation's own README plus the spec.

Nothing in this comment may contain a mustache that is not a real reference.
-->

# terrace-config

A configuration contract for containerised services, machine-checked so a Helm chart's CI can
prove what it renders is still read.

[![Release](https://img.shields.io/github/v/release/TimSchoenle/terrace-config?sort=semver)](https://github.com/TimSchoenle/terrace-config/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/TimSchoenle/terrace-config/ci.yml?branch=main&label=ci)](https://github.com/TimSchoenle/terrace-config/actions/workflows/ci.yml)
[![Licence](https://img.shields.io/github/license/TimSchoenle/terrace-config)](LICENSE)

## What this is

[`spec/`](spec/) is the document: what a producer must refuse, what a consumer may rely on, and
three conformance tiers saying how closely one implementation's reads match another's. It exists
because a chart's CI job holds an image digest and a `config.toml` it rendered itself, with no way
to know whether the two agree. Rename a key in the service without updating the chart, and the
deserialiser ignores what it does not recognise — the pod starts healthy on a compiled default
nobody chose.

## Features

- `spec/v1/contract.schema.json` is a self-contained meta-schema, needing no network fetch to
  resolve. Three conformance tiers say how closely two implementations' reads actually agree, from
  a well-formed document up to byte-identical output; a second language conforms by emitting one
  that passes `terrace-contract conform`, not by re-implementing anything.
- The reference loader's five layers exist for a pod, not a laptop. A directory of key-named files
  survives a mounted Kubernetes `Secret`'s rotation, following the kubelet's `..data` symlink
  swap. `_FILE` indirection reads the same secret without it ever touching the environment, and
  the `reload` feature rebuilds a running service when those files change, with no restart.
- The contract travels with the image. OCI labels (`dev.terrace.config.contract.version`,
  `.path`, `.prefix`) make it discoverable without a running container, a Dockerfile `LABEL`
  block renders straight from it, and `terrace-contract image verify` reads a built image back to
  check it carries what it claims.
- **A Helm chart's CI gets a real gate.** `check` holds a rendered manifest tree to the contracts
  of the images its charts pin, the one failure a values schema and a Kubernetes validator both
  miss: a `ConfigMap` holding a renamed key is a perfectly valid `ConfigMap`. `bindings`,
  `shapes`, `diff`, `coverage`, `secrets` and `pull` cover the rest of what a chart repository
  needs to stay honest about the images it deploys.
- [`cli/`](cli/) depends on no implementation, and CI asserts the absence. Every rendering is a
  pure function of the published document, so it runs unmodified for a producer nobody has
  written yet.

## Implementations

| Implementation | Language | `producer.loader` | What it provides |
|---|---|---|---|
| [`rust/`](rust/README.md) | Rust | `figment` | The reference implementation. Regenerating a case on the same commit reproduces byte-identical output, and the shared conformance corpus in [`spec/v1/conformance/`](spec/v1/conformance/) is authored against it. |
| [`terrace-config-java`](java/README.md) | Java | `terrace-java` | Byte-identical output across its own runs, and the same environment variable names, aliases, and file spellings as the Rust reference for a shared configuration shape, checked against that same corpus. |
| [`terrace-config-spring`](java/README.md#terrace-config-spring-boot) | Java (Spring Boot) | `spring-boot` | Valid, schema-checked contracts under Spring's own relaxed binding, a dialect that deliberately differs from the other two (single-underscore nesting, indexed list variables, no loader-file layers) and is documented rather than reconciled. |

## The spec

[`spec/README.md`](spec/README.md) is the entry point: a directory-versioned, language-neutral
description of the document, with a meta-schema, reference documents and conformance tiers. It is
shared by every implementation and does not belong to any one of them, which is why it lives here
rather than under `rust/` or `java/`.

## Contributing

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org). Each
implementation has its own build and test commands in its own README; a change to `spec/` that
affects more than one implementation ships together with every corpus update it causes, in the
same pull request.

`README.md` is generated. Edit `.github/templates/root-README.md.hbs` instead. CI renders it on
every pull request and commits the result back to the branch, and a push to `main` whose
`README.md` does not match its template fails.

## Security

[SECURITY.md](SECURITY.md) has the reporting instructions. Do not open a public issue for a
vulnerability.

## Licence

`MIT`. See [LICENSE](LICENSE) for the terms.
