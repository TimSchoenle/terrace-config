<!--
Generated from .github/templates/root-README.md.hbs — edit that file, not this one.

CI renders it on every pull request and commits the result back to the branch, the same way it
does for rust/README.md. See .github/workflows/docs.yml.

This is the repository's front door, not a second copy of the crate README: what the project is,
the implementations and their conformance tiers in one table, and one link into each
implementation's own README plus the spec.

Nothing in this comment may contain a mustache that is not a real reference.
-->

# terrace-config

A configuration contract, and the implementations that read it.

[![Release](https://img.shields.io/github/v/release/TimSchoenle/terrace-config?sort=semver)](https://github.com/TimSchoenle/terrace-config/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/TimSchoenle/terrace-config/ci.yml?branch=main&label=ci)](https://github.com/TimSchoenle/terrace-config/actions/workflows/ci.yml)
[![Licence](https://img.shields.io/github/license/TimSchoenle/terrace-config)](LICENSE)

## What this is

A configuration contract — [`spec/`](spec/) — and its implementations. The contract describes one
document: what a producer must refuse, what a consumer may rely on, and three conformance tiers
that say how closely an implementation's reads match another's. This repository is a spec that
happens to have implementations, not an implementation that happens to contain a spec.

## Implementations

| Implementation | Language | `producer.loader` | Tier | Status |
|---|---|---|---|---|
| [`rust/`](rust/README.md) | Rust | `figment` | tier 3 against itself | shipping |
| `terrace-config-java` | Java | `terrace-java` | tier 2 (target) | not started |
| `terrace-config-spring` | Java (Spring Boot) | `spring-boot` | tier 1 (target), divergence documented | not started |

The two Java rows exist to state intent, not to link anywhere: `java/` does not exist in this
repository yet. Once it does, its own README replaces this note and the links above follow.

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

MIT. [LICENSE](LICENSE) has the terms.
