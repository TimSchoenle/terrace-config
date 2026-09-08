# Testing your configuration

The `testing` feature is a jail that arranges both halves of a layer and restores the environment.

The git URL and the tag to pin are in the [README](../README.md#installation).

Every service that loads configuration this way ends up writing the same fixture: a temporary
directory, a secrets file in it, an environment variable pointing at the directory, and a way to
put all of it back afterwards. The `testing` feature is that fixture, written once.

```toml
[dev-dependencies]
terrace-config = { git = "…", tag = "…", features = ["testing"] }
```

```rust,ignore
use terrace_config::{Terrace, testing::Harness};

fn harness() -> Harness {
    Harness::over(Terrace::new("MYAPP_").reserve("MYAPP_PROFILE"))
}

#[test]
fn the_mounted_secret_outranks_the_config_map() {
    harness().run(|jail| {
        jail.config("[database]\nurl = \"postgres://placeholder/app\"\n")?;
        jail.secret("database__url", "postgres://real/app\n")?;

        let config: Config = jail.load()?;
        assert_eq!(config.database.url.expose_secret(), "postgres://real/app");
        Ok(())
    });
}
```

Inside `run`: the temporary directory is the working directory and is deleted when the test
returns, the environment starts empty and is restored afterwards, and jails are serialised
process-wide because the environment is a global.

The closure returns **this crate's own `Error`**. That is the point of the type rather than a
detail. The obvious way to write this fixture is around `figment::Jail`, whose closure returns a
`figment::Error` — a type large enough that clippy's `result_large_err` fires on every test file
that uses it, and which has no `From<std::io::Error>`, so arranging a symlink means converting an
error by hand. Here `?` works on the arrangement and on the load being tested alike.

## What it arranges

Each method sets *both* halves of a layer — the file, and the variable that makes the loader read
it:

| Method | The layer it arranges |
|--------|-----------------------|
| `jail.config(toml)` | TOML at `$MYAPP_CONFIG`, as a single file |
| `jail.fragment(name, toml)` | TOML at `$MYAPP_CONFIG`, as a directory of fragments merged in name order |
| `jail.env_key("auth.jwt_secret", v)` | `MYAPP_AUTH__JWT_SECRET` |
| `jail.secret("auth__jwt_secret", v)` | a key-named file in `$MYAPP_SECRETS_DIR` |
| `jail.indirection("auth.jwt_secret", v)` | `MYAPP_AUTH__JWT_SECRET_FILE`, pointing at a file it writes |

Every name is derived from the loader the harness was built over, never restated. A test that
spells `MYAPP_AUTH__JWT_SECRET_FILE` out by hand keeps passing after `Terrace::file_suffix`
renames the mechanism — while testing a variable the loader no longer reads.

Below that sit `jail.env`, `jail.write`, `jail.create_dir` and `jail.path` for anything the named
methods do not cover, and `jail.terrace()` for a loader with one knob changed:

```rust,ignore
let config: Config = jail.terrace().shadow_policy(ShadowPolicy::LastWins).load()?;
```

## Mounted volumes

A `Volume` builds the shapes Kubernetes actually produces, which is what the three layouts are
for:

```rust,ignore
jail.secrets_volume()
    .file("auth__jwt_secret", "from-the-volume")
    .stray_dir("nested")
    .projected()          // `..data` and a generation directory beside the real keys
    .create()?;
```

- `.plain()` — ordinary files in an ordinary directory. The default.
- `.projected()` — the *names* a projected volume has, with the keys as regular files. Portable,
  and what pins the skipping rules.
- `.symlinked()` — the mount as the kubelet writes it: the keys are symlinks to `..data/<key>`,
  and `..data` is a symlink to the generation directory. Unix only, so a test using it carries
  `#[cfg(unix)]`.

The last two are not stylistic variants. `.projected()` stayed green while every service in a
cluster booted on compiled defaults, because `DirEntry::metadata()` does not follow symlinks and
reported every real key as "not a file". Only `.symlinked()` reproduces that.

`jail.config_volume()` is the same builder wired to `$MYAPP_CONFIG`, and `jail.volume(dir)` is
one wired to nothing — a decoy, for asserting that a *renamed* variable is the only one being
read.

## Reloading in a test

With `reload` on as well, `Rebuilds` records what the supervisor handed the build closure:

```rust,ignore
harness().run(|jail| {
    jail.secret("database__url", "postgres://one/app")?;
    let boot = jail.load_watched::<Config>()?;
    let loader = jail.terrace();
    let files = jail.sandbox();          // a handle a spawned task can hold
    let rebuilds: Rebuilds = Rebuilds::new();

    jail.block_on(async {
        let shutdown = CancellationToken::new();
        let driver = rebuilds.clone();
        let stop = shutdown.clone();

        tokio::spawn(async move {
            driver.wait_for(1).await;
            files.write("secrets/database__url", "postgres://two/app").expect("rotate");
            driver.wait_for(2).await;
            stop.cancel();
        });

        terrace_config::reload::run(
            (boot.value, boot.sources),
            &shutdown,
            || loader.load_watched().map(|l| (l.value, l.sources)).map_err(ServiceError::from),
            rebuilds.serving(|c: &Config| c.database.url.expose_secret().to_owned()),
        )
        .await
        .expect("the supervisor returns when shutdown is cancelled");
    });

    assert_eq!(rebuilds.seen(), ["postgres://one/app", "postgres://two/app"]);
    Ok(())
});
```

`rebuilds.stays_at(2, window)` is the other assertion, and the one most supervisor tests are
really making: a reload that fails to load, or that resolves to the values already running, must
leave the running service alone. `ServiceError` is the error type `reload::run` asks a service
for, and `rebuilds.serving(…)` is a build closure that records and then serves until it is
cancelled — returning early instead looks right and ends the supervisor, so the test would pass
its first assertion and never see a reload.
