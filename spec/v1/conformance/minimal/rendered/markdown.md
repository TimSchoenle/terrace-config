| Variable | Role | Default | Purpose |
|---|---|---|---|
| `MINIMAL_CONFIG` | config | `config.toml` | Names the TOML layer: a file, or a directory whose `*.toml` files are all merged in name order. |
| `MINIMAL_SECRETS_DIR` | secrets dir | — | Names a directory of key-named files — a mounted Kubernetes `Secret` volume. Each file supplies the key its name spells. |

| TOML | Type | Environment | Default | Flags | Purpose |
|---|---|---|---|---|---|
| `dist_dir` | `String` | `MINIMAL_DIST_DIR` | `public` | — | Bundle directory the readiness probe checks. |
