# Reloading

The `reload` feature rebuilds a running service when the files its configuration came from change.

A `Secret` or `ConfigMap` mounted as a volume is updated in place by the kubelet: a new
timestamped directory is written and `..data` is renamed over the old one. That is the only way a
long-lived process learns a credential was rotated, since environment variables are fixed for the
life of a process.

`reload::run` takes the closure that builds your whole runtime and re-runs it whenever the
watched directories change and then go quiet:

```rust,ignore
use std::sync::Arc;
use terrace_config::Terrace;
use tokio_util::sync::CancellationToken;

fn layers() -> Terrace {
    Terrace::new("MYAPP_")
}

#[tokio::main]
async fn main() -> Result<(), ServiceError> {
    let boot = layers().load_watched::<Config>()?;
    let shutdown = CancellationToken::new();

    terrace_config::reload::run(
        (boot.value, boot.sources),
        &shutdown,
        // Called once per debounced change.
        || {
            layers()
                .load_watched::<Config>()
                .map(|loaded| (loaded.value, loaded.sources))
                .map_err(ServiceError::from)
        },
        // Called once per generation, with a token cancelled when this one must stop.
        |config: Arc<Config>, token: CancellationToken| serve(config, token),
    )
    .await
}
```

Your error type needs `From<reload::WatchError>` and `Display`; nothing else:

```rust,ignore
#[derive(Debug, thiserror::Error)]
enum ServiceError {
    #[error("{0}")]
    Watch(#[from] terrace_config::reload::WatchError),
    #[error("configuration: {0}")]
    Config(String),
}
```

Behaviour worth knowing:

- **`build` must return once it has stopped.** The replacement is not built until the old future
  completes, so the previous listener has released its address before the new one binds it.
- **Everything `build` constructs is rebuilt** — pool, state, router, listener, background tasks.
  Process-global installations made before `run` (a `tracing` subscriber, a metrics recorder) are
  not, and changing the configuration that drives those still needs a restart.
- **A failed or no-op reload changes nothing.** If the new configuration cannot be loaded, or
  resolves to the same values already running, the running service is left exactly as it is and
  the reason is logged.
- **Changes are debounced** for 500 ms by default, since one logical volume update fires several
  filesystem events. Use `reload::run_with` and `reload::Debounce` to choose another window.

Change detection compares the merged figment value, not your config struct — a struct holding a
`secrecy::SecretString` cannot implement `PartialEq` at all.

## Keys only a restart applies

Not every value a rebuild re-reads is one it should apply. A key read before `reload::run` — the
`tracing` filter, the metrics endpoint — is never applied by a rebuild, and one whose change under
live traffic is unsafe, such as a storage backend, should not be. A deployment that rolls its pods
on every change never notices; one that relies on in-place reloads leaves those keys changed on
disk and never applied.

So the contract says which keys are which, and the runtime keeps that promise:

```rust,ignore
use terrace_config::Terrace;
use terrace_config::schema::{Describe, ReloadSupport};

fn layers() -> Terrace {
    // Declared once, on the builder `main` and the contract generator share. Published as
    // `schema.reload`.
    Terrace::new("MYAPP_").reloads(ReloadSupport::rebuild())
}

#[derive(serde::Deserialize, Describe)]
#[config(reload = "live")]          // every key below is applied by a rebuild...
struct Config {
    #[config(reload = "restart")]   // ...except this one, read by the subscriber at start
    log_filter: String,
    #[serde(default)]
    ttl_secs: u64,
}

#[tokio::main]
async fn main() -> Result<(), ServiceError> {
    let (boot, reloader) = layers().reloader::<Config>()?;
    let shutdown = CancellationToken::new();

    terrace_config::reload::run(
        boot.into(),
        &shutdown,
        || reloader.reload().map(Into::into).map_err(ServiceError::from),
        |config: Arc<Config>, token: CancellationToken| serve(config, token),
    )
    .await
}
```

- **`live` is always something you wrote.** A key no `#[config(reload = "…")]` covers is published
  `restart`. On a struct the attribute covers every field and every nested type; on a field, that
  key or everything under it.
- **`restart` wins wherever it is written** — on the field, on its struct, or on a nested type's
  own declaration. A key that must be `live` inside a `restart` struct is expressed by moving the
  `restart` down to the fields that need it.
- **A restart key keeps its boot value across every rebuild.** `Terrace::reloader` captures the
  values at start; each reload re-reads the files, puts every restart key back to its boot value,
  and hands the rebuilt runtime the result. A change to restart keys alone rebuilds nothing.
- **A held-back change is reported.** `Sources::pending_restart` names the keys whose value on disk
  differs from the one in effect, and the supervisor logs a warning whenever that list changes.
- **A restart key's new value is still checked.** A reload extracts the configuration as it is on
  disk before pinning, so a value that would fail the next boot fails this reload loudly now.
- **The declaration and the loader agree or refuse.** `Terrace::reloader` refuses on a loader that
  did not declare a rebuild, and `Terrace::load_watched` refuses on one that did — it would rebuild
  with every key, which is not what the contract says.
- **Reserved keys are always `restart`**, and so is every key under `ReloadSupport::none()`.

What no library can check is that `main` actually runs the supervisor it declared. Keep the
declaration and the call to `reload::run` side by side, as above.
