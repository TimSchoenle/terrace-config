# `reload`

A binary that rebuilds on a change, and the keys a rebuild does and does not apply.

A chart reads this to decide which changes roll its pods. A key published `live` is left out of
the digest the pods roll on, because the running process picks it up from the updated mount; a key
published `restart` is in it. So every `live` here is a claim the process has to keep, and every
`restart` is a change that would otherwise be reported as applied and never be.

## What it pins

| Key | Pins |
|---|---|
| `schema.reload` | `mode: rebuild` over all three file layers, between `loader` and `keys`. |
| `ttl_secs` | `live`, from the container's `#[config(reload = "live")]`. |
| `log_filter` | `restart`, from the field's own attribute inside a `live` container. |
| `profile` | `restart` although the container says `live`: it is `reserved`, read from an environment that cannot change. Refusal 11 in [`FORMAT.md`](../../FORMAT.md#what-a-producer-must-refuse). |
| `storage.dir` | `restart`, from the nested type's own attribute — an explicit `restart` anywhere on the path wins over a `live` anywhere else. |
| `upstream.url`, `upstream.token` | `live` through the root, the second a secret: a rotated credential in a mounted file is the case reloading exists for. |

**Consistency rules a producer is held to here.**

- Once `schema.reload` is published, every key carries `reload`. A key nothing marked would be
  `restart`, stated rather than left to a default.
- No key is `live` unless `schema.reload.mode` is `rebuild` — refusal 10.
- `rebuild` is never published with an empty `layers` — refusal 12.
- A document that says nothing about reloading carries neither field. Every other producer case in
  this corpus is that document, and none of their bytes moved when this case was added.

**In `rendered/`.** Nothing reload-specific. The shared renderings do not show the class, so their
bytes are what they would be for the same keys without it — which is the property this case pins
for them.

## Source

Reference implementation — `tests/spec.rs`:

```rust
#[derive(Deserialize, Serialize, Describe)]
#[config(reload = "live")]
struct Rebuilding {
    /// Seconds a rendered page stays cached. Applied by the next rebuild.
    #[serde(default)]
    ttl_secs: u64,
    /// Read by the `tracing` subscriber, which is installed before the supervisor starts.
    #[config(reload = "restart")]
    #[serde(default = "default_filter")]
    log_filter: String,
    /// Read directly from the environment before the layers exist.
    #[serde(default)]
    profile: String,
    #[config(nested)]
    storage: Storage,
    #[config(nested)]
    upstream: Upstream,
}

#[derive(Deserialize, Serialize, Default, Describe)]
#[config(reload = "restart")]
struct Storage {
    /// Directory the data is kept in.
    #[serde(default)]
    dir: String,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Upstream {
    /// Where requests are forwarded.
    #[serde(default)]
    url: String,
    /// Bearer token for the upstream, rotated by replacing the mounted file.
    #[config(secret)]
    token: Option<String>,
}

Terrace::new("RELAY_")
    .reserve("RELAY_PROFILE")
    .reloads(ReloadSupport::rebuild())
    .schema::<Rebuilding>()
    .with_defaults_from(&Rebuilding::default())?
    .into_contract(App::new("relay").version("1.0.0"))
    .build()?
```

`default_filter()` returns `"info"`; `Rebuilding::default()` sets `ttl_secs` to 60 and is
hand-written for [`minimal`](../minimal/)'s reason.

No Java fixture: `terrace-java` publishes no reload support yet, so it has nothing to compare. Its
codecs round-trip this document byte for byte, which is what `ContractCorpusRoundTripTest` holds
them to.
