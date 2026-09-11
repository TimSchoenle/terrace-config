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
