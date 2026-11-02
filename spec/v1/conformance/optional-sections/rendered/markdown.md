| Variable | Role | Default | Purpose |
|---|---|---|---|
| `LIMITER_CONFIG` | config | `config.toml` | Names the TOML layer: a file, or a directory whose `*.toml` files are all merged in name order. |
| `LIMITER_SECRETS_DIR` | secrets dir | — | Names a directory of key-named files — a mounted Kubernetes `Secret` volume. Each file supplies the key its name spells. |

| TOML | Type | Environment | Default | Flags | Purpose |
|---|---|---|---|---|---|
| `global.per_minute` | `u32` | `LIMITER_GLOBAL__PER_MINUTE` | `300` | — | Requests allowed per minute, which is the bucket's refill rate. |
| `global.burst` | `u32` | `LIMITER_GLOBAL__BURST` | `60` | — | The most requests a client may spend back-to-back. |
| `auth.per_minute` | `u32` | `LIMITER_AUTH__PER_MINUTE` | unset | — | Requests allowed per minute, which is the bucket's refill rate. |
| `auth.burst` | `u32` | `LIMITER_AUTH__BURST` | unset | — | The most requests a client may spend back-to-back. |
| `store.url` | `String` | `LIMITER_STORE__URL` | — | required | Connection URL of the counter store. |
