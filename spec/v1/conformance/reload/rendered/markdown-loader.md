| Variable | Role | Default | Purpose |
|---|---|---|---|
| `RELAY_CONFIG` | config | `config.toml` | Names the TOML layer: a file, or a directory whose `*.toml` files are all merged in name order. |
| `RELAY_SECRETS_DIR` | secrets dir | — | Names a directory of key-named files — a mounted Kubernetes `Secret` volume. Each file supplies the key its name spells. |
| `RELAY_PROFILE` | reserved | — | Read directly from the environment before the layered config exists, so no file may supply it. |
