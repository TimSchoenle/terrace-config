| Variable | Role | Default | Purpose |
|---|---|---|---|
| `RELAY_CONFIG` | config | `config.toml` | Names the TOML layer: a file, or a directory whose `*.toml` files are all merged in name order. |
| `RELAY_SECRETS_DIR` | secrets dir | — | Names a directory of key-named files — a mounted Kubernetes `Secret` volume. Each file supplies the key its name spells. |
| `RELAY_PROFILE` | reserved | — | Read directly from the environment before the layered config exists, so no file may supply it. |

| TOML | Type | Environment | Default | Flags | Purpose |
|---|---|---|---|---|---|
| `ttl_secs` | `u64` | `RELAY_TTL_SECS` | `60` | — | Seconds a rendered page stays cached. Applied by the next rebuild. |
| `log_filter` | `String` | `RELAY_LOG_FILTER` | `info` | — | Read by the `tracing` subscriber, which is installed before the supervisor starts. |
| `profile` | `String` | `RELAY_PROFILE` | `""` | reserved | Read directly from the environment before the layers exist. |
| `storage.dir` | `String` | `RELAY_STORAGE__DIR` | `""` | — | Directory the data is kept in. |
| `upstream.url` | `String` | `RELAY_UPSTREAM__URL` | `""` | — | Where requests are forwarded. |
| `upstream.token` | `String` | `RELAY_UPSTREAM__TOKEN` | unset | secret | Bearer token for the upstream, rotated by replacing the mounted file. |
