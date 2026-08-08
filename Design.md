# Extracting the configuration system

TankoVault's layered configuration loader and its reload supervisor are the most reusable code in
this repository, and nothing equivalent exists on crates.io. This document is the design and
implementation plan for moving them into a standalone repository, `terrace`, and consuming them
back as a git dependency.

It is a plan, not a record: nothing here has been implemented. Where it predicts a gate will
fail, that prediction comes from reading the gate, not from running it.

## Contents

- [1. Why extract this](#1-why-extract-this)
- [2. Identity](#2-identity)
- [3. Scope](#3-scope)
- [4. Crate layout and public API](#4-crate-layout-and-public-api)
- [5. Behaviour changes](#5-behaviour-changes)
- [6. Migrating TankoVault](#6-migrating-tankovault)
- [7. Three things that will break](#7-three-things-that-will-break)
- [8. Repository setup](#8-repository-setup)
- [9. Phases](#9-phases)
- [10. Risk and estimate](#10-risk-and-estimate)

---

## 1. Why extract this

Two independent arguments, and both have to hold or this is not worth doing.

### 1.1 The ecosystem has the pieces but not the composition

A survey of crates.io in August 2026 found nothing that does what `crates/config` plus
`crates/service/src/reload.rs` do together.

**Layering.** [`figment`](https://crates.io/crates/figment) is the base and is what we already
build on; it has no secrets-directory provider and no reload story.
[`figment_file_provider_adapter`](https://github.com/nitnelave/figment_file_provider_adapter)
(105k downloads, last released **October 2023**) implements the `_FILE` suffix half and nothing
else — and resolves a key supplied twice by precedence, which is the behaviour we deliberately
refuse. [`confique`](https://github.com/LukasKalbertodt/confique/) (392k downloads, well
maintained) is a figment *alternative* with no reload.
[`settings_loader`](https://github.com/dmrolfs/settings-loader-rs) has almost exactly our
precedence order and a `.with_secrets(path)`, but 235 recent downloads, one author, and hot
reload listed as a possible future enhancement rather than a feature.

**Watching.** [`notify-debouncer-full`](https://crates.io/crates/notify-debouncer-full) (3.6M
recent downloads, official notify-rs) is the real answer for debouncing and we should adopt it
regardless of this extraction. [`hot_reload`](https://github.com/junkurihara/rust-hot-reloader)
(30k downloads, actively maintained) is the closest single match and converges on two of our
decisions independently — an `Option` return where `None` means unchanged, and a failed reload
that logs and retries rather than terminating. It is nonetheless unusable here: its change
detection requires `V: Eq + PartialEq`, and our config cannot implement that because
`SecretString` has no `PartialEq`. That constraint is precisely why we fingerprint the merged
figment value instead of the typed struct. Its bundled file reloader also watches **files** with
`RecursiveMode::NonRecursive`, which is the Kubernetes bug our `Sources::watch_paths` doc comment
exists to prevent. [`config_watcher`](https://github.com/schaze/config_watcher) targets ConfigMaps
explicitly but has 107 recent downloads;
[`reload_config`](https://docs.rs/reload_config/latest/reload_config/) last released in 2021.

**The supervisor.** Nothing. `tokio-graceful-shutdown` and `soft-cycle` handle lifecycle and
cancellation with no config input. Cloudflare's `shellflip`/`ecdysis` solve the same problem one
level down, with process-level restarts and listener fd handoff — a heavier and more correct
answer to the rebind race, and the wrong trade for a deployment where the scheduler already
handles rollouts.

Three properties have no implementation anywhere:

1. A secrets-**directory** provider that survives a projected Kubernetes volume — `..data`
   symlink traversal, dot-prefix skipping, and `fs::metadata` rather than `DirEntry::metadata()`.
2. Shadow-key **rejection** rather than precedence.
3. Value-fingerprint no-op detection over a struct containing non-`Eq` secret types.

### 1.2 It fixes a real coupling wart here

`crates/config` depends on `tankovault-matcher` and `tankovault-domain`, because `MatchingConfig`
lives there and its defaults are the scorer's own. The consequence is that `services/render` and
`services/challenge-solver` — which go out of their way to avoid the database and the crawl stack
— link the *matcher* just to load a config file. Splitting the loader out of the config structs
makes that visible and fixable, independently of whether anything is reused elsewhere.

---

## 2. Identity

| | |
|---|---|
| **Repository** | `github.com/TimSchoenle/terrace` |
| **Crates** | `terrace-config` (providers + loader), `terrace-reload` (supervisor) |
| **Licence** | MIT |
| **Distribution** | GitHub only — consumed as a git dependency, not published to crates.io |

`terrace` is the umbrella: a stack of level layers cut into a slope, which is the shape of a
layered configuration. It names the repository and prefixes both crates, and it is deliberately
*not* a crate name on its own — the two crates do different jobs and the suffix is what says
which.

The `-config` suffix carries the keyword, which is the part that earns its keep. A pure metaphor
name is invisible to anyone searching for what this does, and for a crate whose entire pitch is
filling one specific ecosystem gap that is a real cost. With the suffix present, the metaphor
stops having to do explanatory work and becomes what it should be — a brand token. It also leaves
the door open: nothing about `terrace-config` is bound to figment, so a second backend later would
not make the name a lie.

`terrace-config` and `terrace-reload` were both free on crates.io at the time of writing. That
matters even under GitHub-only distribution — a name collision anywhere in the dependency graph is
a resolution error, and keeping the names reservable leaves the crates.io option open later.

Note the usual package-versus-crate spelling: the package is `terrace-config`, the Rust path is
`terrace_config`. Both appear in this document and the difference is not a typo.

GitHub repository description — one line, since it is shown under the repository name and in
search listings, where anything past roughly a hundred characters is truncated:

> Layered figment configuration that survives Kubernetes secret rotation.

README tagline, where there is room to say what that means:

> Layered figment configuration that survives mounted-secret rotation — Kubernetes Secret volumes,
> `_FILE` indirection, and a supervisor that rebuilds your service when they change.

Repository topics: `rust`, `configuration`, `figment`, `kubernetes`, `secrets`, `hot-reload`.
The same list, minus `rust`, serves as `keywords` in both manifests.

### 2.1 On the licence

MIT, per decision. It is already on `deny.toml`'s allow list and `about.toml`'s accepted list, so
consuming these crates costs no configuration change in this repository (§7.2). The one thing MIT
alone gives up against the Rust ecosystem's conventional `MIT OR Apache-2.0` dual licence is
Apache-2.0's explicit patent grant; for a configuration loader authored by a single person that is
a theoretical concern rather than a practical one.

Relicensing is uncontested. `git log` over `crates/config/src` and `crates/service/src/reload.rs`
returns 19 commits, all authored by Tim Schönle under two email spellings of one GitHub account.
No CLA and no third-party sign-off are needed.

---

## 3. Scope

### 3.1 What moves

| Source | Lines | Destination |
|---|---|---|
| `crates/config/src/secrets.rs` | 661 | `terrace_config::provider::{SecretsDir, FileSuffixEnv}` |
| `crates/config/src/loader.rs` — `assemble`, `toml_layers`, `load`, `load_watched`, `Loaded`, `Sources` | ~250 of 388 | `terrace_config::{Terrace, Loaded, Sources}` |
| `crates/config/src/error.rs` | 19 | `terrace_config::Error` |
| `crates/service/src/reload.rs` | 361 | `terrace_reload::{run, Source}` |

Roughly 1,300 lines, of which a large fraction is doc comments recording failures that already
happened once. **Those comments move verbatim.** A mechanical extraction that trims them to fit a
tidier public API throws away the thing being reused.

### 3.2 What stays

- Every `*Config` struct in `crates/config`. `DatabaseConfig`, `SecurityConfig`, `MatchingConfig`
  and the rest are TankoVault vocabulary and have no business in a general-purpose crate.
- `loader.rs::default_true()` and `is_production()`. Both are three lines; `is_production` reads
  `TANKOVAULT_PROFILE`, which is a profile convention this project chose, and it is load-bearing
  for `xtask config-docs` (§7.1).
- `crates/config`'s dependency on `tankovault-matcher`. Fixing that means moving `MatchingConfig`
  into `crates/matcher`, which is a behavioural change to a scoring path. Bundling it into a
  mechanical extraction is how a reviewable change becomes an unreviewable one. It is filed as a
  follow-up in §9.4.

### 3.3 What is explicitly not in scope

The wider extraction survey identified four further candidates — an axum hardening crate
(RFC 9457 problems, shutdown, health probes, the middleware ordering), an SSRF guard, a
rate-limiter, and a Postgres test harness. None of them is touched here. One extraction, proven
end to end through a full `xtask ci`, before a second is started.

---

## 4. Crate layout and public API

```
terrace/
├── Cargo.toml                  # workspace
├── LICENSE                     # MIT
├── README.md
├── CHANGELOG.md
├── deny.toml
├── .github/workflows/ci.yml
├── terrace-config/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # Terrace builder, load, load_watched
│       ├── error.rs
│       ├── key.rs              # normalise_key, env_spelling, insert_nested
│       ├── loaded.rs           # Loaded<T>, Sources
│       └── provider/
│           ├── mod.rs
│           ├── toml_layers.rs  # file-or-directory expansion
│           ├── secrets_dir.rs
│           └── file_suffix.rs
└── terrace-reload/
    ├── Cargo.toml
    └── src/lib.rs              # Source trait, run(), Watch, WatchError
```

### 4.1 `terrace-config`

Every environment name is derived from one prefix unless overridden, which is the whole of the
parameterisation this extraction needs.

```rust
/// The layered loader.
///
/// Layers, lowest precedence first: struct defaults, TOML at `$<PREFIX>CONFIG` (a file, or
/// every `*.toml` in it when it names a directory), `<PREFIX>`-prefixed `__`-nested environment
/// variables, `$<PREFIX>SECRETS_DIR`, and `<PREFIX><KEY>_FILE` indirection.
pub struct Terrace { /* … */ }

impl Terrace {
    /// `Terrace::new("MYAPP_")` reads `MYAPP_CONFIG`, `MYAPP_SECRETS_DIR`, `MYAPP_*` and
    /// `MYAPP_<KEY>_FILE`.
    pub fn new(prefix: impl Into<String>) -> Self;

    pub fn config_var(self, name: impl Into<String>) -> Self;
    pub fn secrets_dir_var(self, name: impl Into<String>) -> Self;
    pub fn default_config_path(self, path: impl Into<PathBuf>) -> Self;
    pub fn file_suffix(self, suffix: impl Into<String>) -> Self;      // "_FILE"
    pub fn nesting_separator(self, sep: impl Into<String>) -> Self;   // "__"

    /// A key read from the environment before the layers exist, which a file therefore may
    /// not supply. The config and secrets-directory variables are reserved automatically.
    pub fn reserve(self, key: impl Into<String>) -> Self;

    /// What to do when one key is supplied by two mechanisms. Defaults to `Reject`.
    pub fn shadow_policy(self, policy: ShadowPolicy) -> Self;

    pub fn figment(&self) -> Result<Figment, Error>;
    pub fn load<T: DeserializeOwned>(&self) -> Result<T, Error>;
    pub fn load_watched<T: DeserializeOwned>(&self) -> Result<Loaded<T>, Error>;
}

pub enum ShadowPolicy { Reject, LastWins }

pub struct Loaded<T> { pub value: T, pub sources: Sources }

pub struct Sources { /* watch: Vec<PathBuf>, fingerprint: figment::value::Value */ }

impl Sources {
    /// Directories to watch. Directories, not files: a Kubernetes volume update renames a
    /// whole new `..data` directory over the old one, so a watch registered against a file's
    /// inode never fires a second time.
    pub fn watch_paths(&self) -> &[PathBuf];

    /// Whether `self` resolves to different values than `previous`.
    pub fn differs_from(&self, previous: &Self) -> bool;
}

pub enum Error { Figment(Box<figment::Error>), Invalid(String), Source(String) }
```

The three providers are public and usable against a hand-built `Figment`, for a consumer who wants
the secrets-directory layer without the rest of the loader:

```rust
pub mod provider {
    pub struct TomlLayers { /* … */ }   // impl figment::Provider
    pub struct SecretsDir { /* … */ }
    pub struct FileSuffixEnv { /* … */ }
}
```

### 4.2 `terrace-reload`

```rust
/// Where a configuration came from, and whether it has since changed.
pub trait Source: Sized {
    fn watch_paths(&self) -> &[PathBuf];
    fn differs_from(&self, previous: &Self) -> bool;
}

/// Run a service, rebuilding it whenever its configuration files change.
///
/// `build` receives the current configuration and a token cancelled when the runtime must
/// stop. It should return once it has stopped: the replacement is not built until it does, so
/// the old listener has released the address before the new one binds it.
///
/// A reload that cannot be loaded, or that fails to build, leaves the running service exactly
/// as it was.
pub async fn run<C, S, R, F, Fut, E>(
    boot: (C, S),
    shutdown: &CancellationToken,
    reload: R,
    build: F,
) -> Result<(), E>
where
    S: Source,
    R: Fn() -> Result<(C, S), E>,
    F: Fn(Arc<C>, CancellationToken) -> Fut,
    Fut: Future<Output = Result<(), E>>,
    E: Display + From<WatchError>;

pub struct WatchError(String);

/// How long the filesystem must be quiet before a change is acted on. Default 500ms.
pub struct Debounce(pub Duration);
```

`terrace-config` provides `impl Source for Sources`, and that impl is the only line connecting the
two crates — **`terrace-reload` does not depend on `terrace-config`**. The supervisor is useful to anyone
with a `Fn(Arc<C>, CancellationToken) -> Future` and a way to detect change, regardless of how
they load configuration; coupling it to figment would shrink its audience for no benefit.

**Why a trait rather than a fingerprint type parameter.** The fingerprint has to be
`figment::value::Value`, because comparing the typed config struct is impossible — several config
structs hold `SecretString`, which deliberately has no `PartialEq`. A `F: PartialEq` type
parameter would work and would keep figment out of the crate, but it leaks the mechanism into
every caller's signature. The trait hides it in one impl on the one type that has it.

---

## 5. Behaviour changes

| # | Change | Why |
|---|---|---|
| 1 | `notify-debouncer-full` replaces the hand-rolled `DEBOUNCE` / `SIGNAL_DEPTH: 1` / `while try_recv()` drain | Maintained, 3.6M recent downloads, official notify-rs. Removes ~40 lines of the trickiest code in `reload.rs`. |
| 2 | The debounce window becomes a parameter, default 500ms (unchanged) | A crate cannot hardcode one deployment's kubelet sync period. |
| 3 | `ShadowPolicy` | Adoption — see below. |
| 4 | Providers exported individually | A consumer with an existing figment wants `SecretsDir` alone. |
| 5 | `reload` becomes a caller-supplied closure instead of a hard call to `tankovault_config::load_watched` | The one project coupling left in `reload.rs`. |
| 6 | Every `TANKOVAULT_`-shaped constant becomes a builder field | The point of the exercise. |

`ShadowPolicy::LastWins` is new and exists purely so the crate is adoptable: anyone migrating from
`figment_file_provider_adapter` has precedence semantics today and will not switch to a crate that
fails their boot. `Reject` remains the default and keeps its doc comment intact — a stale
environment variable shadowing a rotated mounted secret keeps the service running on the old
credential, and the discrepancy surfaces during an incident rather than during a deploy.

**Caveat on change 1.** `notify-debouncer-full`'s current release is `0.8.0-rc.2` (May 2026). This
workspace already carries three pre-release pins — `wreq`, `wreq-util` and `webauthn-rs`
`=0.6.1-dev` — each for a reason recorded in the root `Cargo.toml`. A debouncer is not such a
reason. Check whether the stable 0.5/0.6 line covers the API needed; if it does, take stable. If
it does not, keep the hand-rolled debounce for v0.1 and revisit rather than adding a fourth rc.

**Not changing.** The `..data` symlink traversal via `fs::metadata`; watching directories rather
than files; values emitted as unparsed strings, because `figment::providers::Env` TOML-parses
values and an all-digit password would otherwise fail to deserialise into `SecretString`; a
`_FILE` naming an unreadable path being fatal rather than skipped; and the failed-reload-keeps-
running posture. Those are the reasons the crate exists.

---

## 6. Migrating TankoVault

The design goal is **zero churn under `services/*/src/main.rs`**. All nine services call
`tankovault_config::load()`, `tankovault_config::load_watched()` and
`tankovault_service::run_reloading()`; those three signatures stay identical.

### 6.1 `crates/config`

`lib.rs` gains the one place the TankoVault dialect is spelled:

```rust
/// Keys read straight from the environment, before or outside the layered config.
///
/// Spelled as literals rather than derived, because `xtask config-docs` scans for them
/// textually to build the documented surface; see docs/CONFIG_EXTRACTION.md §7.1.
const RESERVED: [&str; 2] = ["TANKOVAULT_PROFILE", "TANKOVAULT_CONFIRM_RESET"];

fn layers() -> &'static Terrace {
    static LAYERS: LazyLock<Terrace> = LazyLock::new(|| {
        RESERVED
            .iter()
            .fold(Terrace::new("TANKOVAULT_"), |t, k| t.reserve(*k))
    });
    &LAYERS
}

pub fn load<T: DeserializeOwned>() -> Result<T, ConfigError> {
    layers().load()
}

pub fn load_watched<T: DeserializeOwned>() -> Result<Loaded<T>, ConfigError> {
    layers().load_watched()
}

pub use terrace_config::{Loaded, Sources};
pub type ConfigError = terrace_config::Error;
```

- Delete `secrets.rs` entirely, tests included.
- Delete `loader.rs`'s `assemble`, `toml_layers`, `Loaded`, `Sources` and the layer tests. Keep
  `default_true()` and `is_production()`.
- Drop the `figment` dev-dependency if no remaining test needs `Jail`.

Net: roughly **1,050 lines leave `crates/config`**.

### 6.2 `crates/service`

`reload.rs` collapses to an adapter over `terrace_reload::run`:

```rust
pub async fn run<C, F, Fut, E>(
    boot: Loaded<C>,
    shutdown: &CancellationToken,
    build: F,
) -> Result<(), E>
where
    C: DeserializeOwned,
    F: Fn(Arc<C>, CancellationToken) -> Fut,
    Fut: Future<Output = Result<(), E>>,
    E: std::fmt::Display + From<ServiceError>,
{
    terrace_reload::run(
        (boot.value, boot.sources),
        shutdown,
        || tankovault_config::load_watched::<C>().map(|l| (l.value, l.sources)),
        build,
    )
    .await
}
```

`ServiceError::Watch(String)` gains `From<terrace_reload::WatchError>`. The `run_reloading`
re-export in `lib.rs` is unchanged. `notify` leaves the crate's manifest. Net: roughly **340 lines
leave `crates/service`**.

### 6.3 Everything else

| Consumer | Change |
|---|---|
| `services/*/src/main.rs` (×9) | **None.** |
| `fuzz/fuzz_targets/config_env_load.rs` | None — it fuzzes `tankovault_config::load`, which still exists. Mirror a copy into the `terrace` repo. |
| `web/frontend` | None — separate workspace, no config dependency. |
| `xtask config-docs` | See §7.1. |
| `deny.toml`, `about.toml`, `THIRD-PARTY-NOTICES` | See §7.2 and §7.3. |

---

## 7. Three things that will break

### 7.1 `xtask config-docs` loses two documented keys

`xtask/src/config_docs/surface.rs` derives part of the documented surface by **scanning textually
for `env::var("TANKOVAULT_…")` string literals**. Today `TANKOVAULT_CONFIG` comes from
`loader.rs:109` and `TANKOVAULT_SECRETS_DIR` from `secrets.rs:81`. After extraction, both live in
`terrace-config`, where the names are *derived from a prefix* and no literal exists.

The consequence is that `cargo run -p xtask -- config-docs --check` fails, reporting the two keys
as stale rows in `docs/CONFIGURATION.md`. That is a real regression in a gate that exists
precisely because an unknown `TANKOVAULT_*` key is ignored at runtime rather than rejected, which
makes drift between the document and the code silent.

**Fix, in the same commit.** Extend `direct_env_keys` with a second recogniser for a
`RESERVED`-shaped `const [&str; N]` array in `crates/config`, and declare all four process keys
there (§6.1 shows two; `TANKOVAULT_CONFIG` and `TANKOVAULT_SECRETS_DIR` join them). Then extend
the expected list in `the_derived_surface_is_the_whole_surface` to include `TANKOVAULT_CONFIG`, so
this cannot regress silently a second time. `TANKOVAULT_PROFILE` survives either route, via
`is_production()`.

### 7.2 `cargo deny` and the notices will see both crates

`deny.toml` carries `private = { ignore = true }`, which skips licence-checking **workspace
members**. A git dependency is not a workspace member. Both `terrace-config` and `terrace-reload`
therefore enter the licence check and `THIRD-PARTY-NOTICES` like any third-party crate.

Under MIT this costs nothing: MIT is already in `deny.toml`'s `[licenses] allow` and in
`about.toml`'s `accepted` list, which `xtask repo-lint` holds equal to each other. Both crates
render as ordinary permissive sections in the notices document. Had the licence been
PolyForm-Noncommercial — this workspace's own — it would have required an edit to both lists and a
permanently weakened allow list, which is the main reason the licence question had to be settled
before the split rather than after.

### 7.3 `Cargo.lock` moves, so notices regeneration is mandatory

Per the working agreement, `Cargo.lock` moving means running `cargo run -p xtask -- notices`,
which needs `cargo-about` installed. Verify during Phase 3 that `cargo-about` resolves a licence
file out of a **git** checkout rather than only a registry one. If it cannot, the notices gate
fails and the fix is a `clarify` entry for each crate in `about.toml`.

Both lockfiles are in play: the root workspace and `fuzz`, which carries its own. `web/frontend`
is unaffected.

---

## 8. Repository setup

| Item | Decision |
|---|---|
| Licence | MIT, `LICENSE` at root, `license = "MIT"` in both manifests |
| `publish` | `publish = false` on both crates — prevents an accidental `cargo publish` while leaving the names reservable |
| MSRV | 1.94, matching this workspace's `rust-version` |
| Edition | 2024 |
| Lints | Copy this workspace's `[workspace.lints]` verbatim: `unsafe_code = "forbid"`, `pedantic`, `allow_attributes = "warn"`, `allow_attributes_without_reason = "warn"`, `missing_errors_doc`, `missing_panics_doc`, `broken_intra_doc_links = "deny"` |
| CI | The shared `TimSchoenle/actions` preset — harden-runner, SHA-pinned actions, zizmor. Jobs: `fmt`, `clippy`, `test` on Linux **and** Windows, `doc`, `deny`, `msrv` |
| Renovate | The same shared preset |
| Versioning | Hand-cut git tags, `v0.1.0` onward. Release-please is overhead for a two-crate repository with one consumer |

Windows is in the test matrix deliberately. The ConfigMap-symlink tests are `#[cfg(unix)]` and the
mount scenario does not arise on Windows, but development happens there and the crate must at
minimum build and pass everything else.

### 8.1 Consuming it

```toml
# Cargo.toml, [workspace.dependencies]
terrace-config = { git = "https://github.com/TimSchoenle/terrace", tag = "v0.1.0" }
terrace-reload = { git = "https://github.com/TimSchoenle/terrace", tag = "v0.1.0" }
```

Pin by **tag**, not branch. `Cargo.lock` records the resolved revision either way, but a branch
dependency means `cargo update` moves silently across arbitrary commits, whereas a tag makes every
bump a deliberate manifest edit that appears in review. Confirm the shared Renovate preset handles
git-tag dependencies; if it does not, that is a one-entry `packageRules` addition.

Git dependencies are forbidden in crates published to crates.io. That is irrelevant here — every
crate in this workspace is already `publish = false`.

---

## 9. Phases

### 9.1 Phase 1 — stand up the repository

No TankoVault changes.

1. Create `TimSchoenle/terrace`: workspace skeleton, `LICENSE`, CI from the shared preset.
2. Move `secrets.rs` to `provider/secrets_dir.rs` and `provider/file_suffix.rs`, mechanically,
   comments intact.
3. Move `toml_layers` to `provider/toml_layers.rs`.
4. Build the `Terrace` builder; replace every `TANKOVAULT_`-shaped constant and the `PROCESS_KEYS`
   array with a field.
5. Move `Loaded`, `Sources` and `Error`.
6. Port every test, parameterising the prefix to something neutral such as `TEST_`. The
   `#[cfg(unix)]` test that builds a real ConfigMap volume out of symlinks is the one that must
   not be lost — it is what caught `DirEntry::metadata()` not traversing symlinks, after the
   regular-file version of the same test stayed green while every service in the cluster loaded an
   empty config layer.
7. Add `ShadowPolicy`, with a test per arm.

**Done when:** `cargo test` green on Linux and Windows, `cargo doc` clean, and the README example
loads a config from a fake secrets directory.

### 9.2 Phase 2 — `terrace-reload`

1. Move `reload.rs`; define `Source`; replace the hard `load_watched` call with the `reload`
   closure.
2. Swap the hand-rolled debounce for `notify-debouncer-full`, subject to the stable-line check in
   §5.
3. Port both watcher tests and, above all,
   `a_rotated_secret_rebuilds_and_a_broken_reload_does_not`. That test is the crate's entire
   safety argument: it proves both that a rotated secret rebuilds with the new value exactly once
   and that a reload which fails to load leaves a serving runtime alone. It uses `figment::Jail`,
   so `terrace-config` and `figment/test` become dev-dependencies of `terrace-reload` — dev only,
   so the production dependency direction stays clean.
4. Tag `v0.1.0`.

**Done when:** the rotate-rebuild test passes against `terrace-config`, and `terrace-reload`'s
`[dependencies]` table contains no figment.

### 9.3 Phase 3 — migrate TankoVault

1. Add both git dependencies to `[workspace.dependencies]`.
2. Rewrite `crates/config/src/lib.rs` per §6.1; delete `secrets.rs`; trim `loader.rs`.
3. Collapse `crates/service/src/reload.rs` per §6.2; add the `From<WatchError>` impl; drop
   `notify`.
4. Extend `xtask config-docs`'s `direct_env_keys` per §7.1, and extend its test's expected list.
5. Run `cargo run -p xtask -- notices`; verify `cargo-about` handles the git checkout.
6. Run `config-docs --check`, `repo-lint`, then the full `cargo run -p xtask -- ci`.

**Done when:** `xtask ci` is green with **zero diff** under `services/*/src/main.rs`. If any
service main changed, the wrapper signatures drifted — fix the wrapper, not the service.

### 9.4 Phase 4 — follow-ups

Separate pull requests, none of them blocking.

- Mirror `fuzz/fuzz_targets/config_env_load.rs` into `terrace-config` as a fuzz target of its own.
- Move `MatchingConfig` into `crates/matcher`, cutting `crates/config`'s dependency on the matcher
  and stopping `render` and `challenge-solver` linking it (§1.2).
- Write the README's comparison section: `figment_file_provider_adapter`, `hot_reload` and
  `settings_loader`, and exactly which of the three properties in §1.1 each one lacks. That is the
  crate's reason to exist and belongs where a reader will find it.

---

## 10. Risk and estimate

Roughly **1.5 to 2 days** of focused work. Phase 1 is about a day — the parameterisation and test
porting are the bulk of it. Phase 2 is a few hours. Phase 3 is a few hours plus gate-chasing.

The concentrated risk is Phase 3's gate cascade. `config-docs`, `deny`, `about.toml`, `notices` and
`repo-lint` all react to a change in the dependency graph, and §7 is a prediction of which ones
fire, derived from reading them rather than from running them. Expect one or two that this
document does not foresee.

The second risk is subtler and worth naming: a shared crate freezes an API that currently changes
freely. `crates/config`'s loader has been edited 19 times; once it lives behind a git tag, each of
those edits becomes a tag bump and a manifest change in this repository. That cost is acceptable
here because the loader's surface has been stable for most of those commits — the churn was in the
config *structs*, which are staying — but it is the reason to extract one thing and watch it for a
release cycle before extracting the other four.