# Contributing

This is a multi-language repository: `spec/` is the shared, language-neutral contract, and each
directory under it is an implementation that conforms to that contract. Today that is `rust/`
only; `java/` is planned but does not exist yet.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org). The type decides both the changelog
section and the version bump release-please proposes for the package the change touches.

## Changing the spec

`spec/` is shared. A change to `spec/v1/FORMAT.md`, `spec/v1/CONFORMANCE.md` or the meta-schema
that affects more than one implementation ships together with every corpus update it causes, in
the same pull request — not as a follow-up. `spec/v1/CONFORMANCE.md` explains why: a re-blessed
corpus that lands separately from the spec change it was caused by lets the two drift.

## Building and testing an implementation

Each implementation documents its own commands in its own README. Today:

| Implementation | Where | Commands |
|---|---|---|
| Rust | [`rust/README.md`](../rust/README.md#contributing) | `cargo fmt`, `cargo clippy`, `cargo test`, `cargo deny check`, the fuzz suite |

Java's row is added once `java/` exists (see the Java skeleton work).

## Root-level files

- `README.md` is generated from [`.github/templates/root-README.md.hbs`](../.github/templates/root-README.md.hbs).
  Edit the template, not the file; CI renders it on every pull request.
- `rust/README.md` is generated the same way, from [`.github/templates/README.md.hbs`](../.github/templates/README.md.hbs).
- `docs/` at the root holds repository-level documents only — this file and the migration plan
  and progress notes. Documents about a specific implementation live under that implementation's
  own `docs/` directory instead (for example `rust/docs/`).
