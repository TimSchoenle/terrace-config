# Migration progress

Tracks the state of the move to a multi-language repository (Rust under `rust/`, `spec/` staying
at the root, Java implementations to follow under `java/`). Updated as work lands; each entry
states what changed and what is still open, so a later session can pick up here without needing
the history that produced it.

## Done

- **The crate moved under `rust/`.** `src/`, `macros/`, `tests/`, `examples/`, `fuzz/`,
  `Cargo.toml`, `deny.toml`, `CHANGELOG.md` and `README.md` now live there. `docs/` split: the
  crate's own documents (`ALTERNATIVES.md`, `CONTRACT.md`, `EXPLAIN.md`, `RELOAD.md`, `SCHEMA.md`,
  `TESTING.md`, `config-contract-plan.md`) moved to `rust/docs/`; this file and other repo-level
  documents stay at the root. `spec/`, `LICENSE`, `SECURITY.md`, `.github/`, `.gitignore`,
  `.gitattributes` and `renovate.json` did not move.
- **`rust/tests/spec.rs`** — `spec_dir()` now climbs one level from `CARGO_MANIFEST_DIR` before
  joining `spec/v1`, since the manifest directory is `rust/` and not the repository root.
- **Cross-references updated** in `rust/docs/CONTRACT.md`, `spec/README.md`,
  `spec/v1/FORMAT.md` and `spec/v1/CONFORMANCE.md` to point through `rust/` where the target
  moved.
- **CI (`.github/workflows/ci.yml`)** — every Rust job (`fmt`, `clippy`, `test`, `features`,
  `doc`, `fuzz`, `deny`, `msrv`) now runs against `rust/` (via `working-directory`,
  `manifest-path`, or an explicit `cd`-equivalent). A new **`consumer-smoke-test`** job builds a
  throwaway crate depending on the checkout by git URL and is wired into the aggregate `ci` gate
  permanently, so a subdirectory package that stops resolving fails CI instead of being discovered
  after a tag.
- **`.github/workflows/update-files.yaml`** — lockfile resolution now targets
  `rust/Cargo.toml` and `rust/fuzz/Cargo.toml`, and the commit step's file pattern matches.
- **`.github/workflows/docs.yml`** and **`.github/scripts/readme-variables.sh`** — the manifest
  read defaults to `rust/Cargo.toml`, and the README render/check targets `rust/README.md`.
- **`.github/templates/README.md.hbs`** — now the crate template rendering to
  `rust/README.md`; its MSRV badge, `spec/`, `LICENSE`, `SECURITY.md` and `ci.yml` links were
  re-pointed for that new location.
- **Release automation** — `release-please-config.json` and `.release-please-manifest.json`
  key the Rust package as `"rust"` instead of `"."`, keeping `include-component-in-tag: false` so
  released tags stay `vX.Y.Z`.
- **`renovate.json`** — the fuzz-dependency grouping rule now matches
  `rust/fuzz/Cargo.toml`.
- **Verified locally**, from `rust/`: `cargo fmt --all --check`, `cargo clippy --workspace
  --all-features --all-targets`, `cargo test --workspace --all-features`, and
  `cargo test --all-features --test spec` all pass, including the corpus-vs-render comparison that
  proves the format still resolves `spec/v1/` correctly from the new manifest directory.

## Open

- **Upstream composite actions.** `TimSchoenle/actions/actions/rust/{clippy,test,cargo-check}`
  are now invoked with a `working-directory: rust` input in `ci.yml`. Whether that input actually
  exists in the referenced action version could not be confirmed from this session (the actions
  repository was unreachable). Confirm before merging; if the input is not honoured, the fallback
  is inlining plain `cargo` steps for those jobs and losing the shared hardening, as already noted
  inline where relevant.
- **`docs.yml`'s `readme-variables` action** was given `manifest: rust/Cargo.toml` and
  `docs-directory: rust/docs` inputs on the same speculative basis — confirm the input names
  against the action actually in use.
- **The root `README.md`**, its template, multi-package `release-please` wiring beyond the key
  rename, Java CI jobs and path filters, and Renovate's Maven manager are not started. The
  repository currently has no root `README.md` — expected only until that lands.
- **No Java work has started.** `java/` does not exist yet.
