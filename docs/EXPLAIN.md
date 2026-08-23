# Debugging where a value came from

The `explain` feature reports which layer supplied each key, while holding no configuration value.

The git URL and the tag to pin are in the [README](../README.md#installation).

`load` returns a `T` and throws the provenance away, which is fine right up until a value arrives
from the layer nobody expected. The `explain` feature keeps it.

```toml
terrace-config = { git = "…", tag = "…", features = ["explain"] }
```

```rust
use terrace_config::Terrace;

let terrace = Terrace::new("MYAPP_");

// At boot, and again from inside a reload — it re-reads at the moment it is called.
println!("{}", terrace.explain()?);
```

```text
terrace-config: prefix `MYAPP_`, 4 keys, 1 supplied by more than one layer
layers, lowest precedence first:
  TOML          MYAPP_CONFIG=/etc/myapp/conf.d
                  /etc/myapp/conf.d/10-base.toml (2 keys)
                  /etc/myapp/conf.d/20-tuning.toml (1 key)
  environment   MYAPP_* (1 key)
  secrets dir   MYAPP_SECRETS_DIR=/run/secrets (1 key)
  indirection   MYAPP_*_FILE (none)
keys:
  auth.jwt_secret  <- secrets file /run/secrets/auth__jwt_secret
  database.url     <- environment MYAPP_DATABASE__URL
                      shadowing TOML /etc/myapp/conf.d/10-base.toml
  server.port      <- TOML /etc/myapp/conf.d/20-tuning.toml
  server.workers   <- TOML /etc/myapp/conf.d/10-base.toml
```

That is "why is my mounted secret not being picked up" answered without a debugger: the mount is
listed, and so is the stale variable sitting on top of it. `ShadowPolicy::Reject` refuses that
particular pair at load time, but it deliberately says nothing about the TOML layer — a
checked-in `config.toml` overridden by an environment variable is an ordinary, intended override,
ordinary until it is the one you did not know about.

Three properties are worth knowing before you wire it into a boot log:

- **It holds no configuration value.** Not redacted on the way out — never recorded. There is no
  field to leak, so `Display`, `Debug` and anything built from the accessors are safe by
  construction. The one thing this costs is that an unparseable TOML fragment is reported as
  `not valid TOML` with no reason attached: a parse error quotes the line it failed on, and that
  line can be the credential. `load` fails with figment's own message, which is where the detail
  belongs.
- **It does not fail for the reason you are running it.** `explain` assembles under
  `ShadowPolicy::LastWins` whatever policy you set, so a configuration `load` *refuses* can still
  be explained, and the doubly-supplied key is reported as one key with two sources.
- **It reports what the environment did**, not what the type can carry — keys nothing supplied
  are absent. [`schema`](SCHEMA.md) answers the other half, and answers it without reading anything.

For a machine rather than a log, walk it instead of printing it:

```rust
use terrace_config::Terrace;

let explanation = Terrace::new("MYAPP_").explain()?;
for origin in explanation.contested() {
    eprintln!("{} comes from {}", origin.key(), origin.effective());
    for overridden in origin.shadowed() {
        eprintln!("  overriding {overridden}");
    }
}
```

With the `testing` feature it is on the jail too, which is what lets a test assert the layer and
not only the value — a `jail.secret(…)` that a leftover `jail.env_key(…)` is shadowing loads
perfectly well and is testing nothing:

```rust,ignore
harness().run(|jail| {
    jail.secret_key("auth.jwt_secret", "mounted")?;

    let origin = jail.explain()?.origin("auth.jwt_secret").expect("reported");
    assert!(matches!(origin.effective(), Layer::SecretsFile(_)));
    Ok(())
});
```
