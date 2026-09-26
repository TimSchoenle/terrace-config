| Variable | Role | Default | Purpose |
|---|---|---|---|
| `POLICY_CONFIG` | config | `config.toml` | Names the TOML layer: a file, or a directory whose `*.toml` files are all merged in name order. |
| `POLICY_SECRETS_DIR` | secrets dir | — | Names a directory of key-named files — a mounted Kubernetes `Secret` volume. Each file supplies the key its name spells. |

| TOML | Type | Environment | Default | Flags | Purpose |
|---|---|---|---|---|---|
| `documents` | `BTreeMap<String, Policy>`, at `*`: exactly one of: [`url` is set; `body` is not empty], at `*`: when `consent.requirement` is set and not "none": all of: [`url` is not set; `consent.version` matches `[^\\t\\n\\u000B\\f\\r \\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000]`; when `consent.grace_days` is above 0: `consent.effective` is set] | `POLICY_DOCUMENTS` | `{ privacy = { body = { en = We collect nothing. }, consent = { effective = 2026-09-22, grace_days = 14, requirement = accept, version = 1 } }, terms = { body = {  }, consent = { grace_days = 0, requirement = none }, url = https://example.com/terms } }` | — | Documents, by slug. |
