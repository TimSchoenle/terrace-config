| Variable | Role | Default | Purpose |
|---|---|---|---|
| `SITE_CONFIG` | config | `config.toml` | Names the TOML layer: a file, or a directory whose `*.toml` files are all merged in name order. |
| `SITE_SECRETS_DIR` | secrets dir | — | Names a directory of key-named files — a mounted Kubernetes `Secret` volume. Each file supplies the key its name spells. |

| TOML | Type | Environment | Default | Flags | Purpose |
|---|---|---|---|---|---|
| `dist_dir` | `String` | `SITE_DIST_DIR` | `public` | — | Bundle directory the readiness probe checks. |
| `legal.documents` | `BTreeMap<String, LegalDocument>`, must contain: `imprint`, `privacy` | `SITE_LEGAL__DOCUMENTS` | — | required | Legal documents, by the name the site links them under. |
| `legal.links` | `BTreeMap<String, String>`, must contain: `home` | `SITE_LEGAL__LINKS` | `{ home = / }` | — | Footer links, by label. |
