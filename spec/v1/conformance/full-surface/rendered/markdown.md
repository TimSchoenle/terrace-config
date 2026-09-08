| Variable | Role | Default | Purpose |
|---|---|---|---|
| `PORTFOLIO_CONFIG` | config | `config.toml` | Names the TOML layer: a file, or a directory whose `*.toml` files are all merged in name order. |
| `PORTFOLIO_SECRETS_DIR` | secrets dir | — | Names a directory of key-named files — a mounted Kubernetes `Secret` volume. Each file supplies the key its name spells. |
| `PORTFOLIO_PROFILE` | reserved | — | Read directly from the environment before the layered config exists, so no file may supply it. |

| TOML | Type | Environment | Default | Flags | Purpose |
|---|---|---|---|---|---|
| `dist_dir` | `String` | `PORTFOLIO_DIST_DIR` | `public` | — | Bundle directory the readiness probe checks. |
| `api_base` | `String` | `PORTFOLIO_API_BASE` | — | required | Base URL every outbound request is resolved against. |
| `log_level` | `LogLevel`: `trace` \| `debug` \| `info` \| `warn` | `PORTFOLIO_LOG_LEVEL` | `info` | — | How much the service says. |
| `sample_rate` | `f64` | `PORTFOLIO_SAMPLE_RATE` | `0` | — | Fraction of requests sampled for tracing. |
| `methods` | `Vec<Method>` | `PORTFOLIO_METHODS` | `[]` | — | Methods every route admits. |
| `github.username` | `String` | `PORTFOLIO_GITHUB__USERNAME` | — | required | User whose repositories are listed. |
| `github.token` | `String` | `PORTFOLIO_GITHUB__TOKEN` | unset | secret | Bearer token lifting the API rate limit. |
| `github.ttl_secs` | `u64` | `PORTFOLIO_GITHUB__TTL_SECS` | `0` (permanent) | — | Revalidation interval in seconds. |
