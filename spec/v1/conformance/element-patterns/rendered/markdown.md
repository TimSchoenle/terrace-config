| Variable | Role | Default | Purpose |
|---|---|---|---|
| `BOOK_CONFIG` | config | `config.toml` | Names the TOML layer: a file, or a directory whose `*.toml` files are all merged in name order. |
| `BOOK_SECRETS_DIR` | secrets dir | — | Names a directory of key-named files — a mounted Kubernetes `Secret` volume. Each file supplies the key its name spells. |

| TOML | Type | Environment | Default | Flags | Purpose |
|---|---|---|---|---|---|
| `default_locale` | `String`, matches `^[A-Za-z]{2,3}(?:[-_][A-Za-z]{4})?(?:[-_](?:[A-Za-z]{2}\|[0-9]{3}))?$` | `BOOK_DEFAULT_LOCALE` | `en` | — | The locale a chapter falls back to. |
| `chapters` | `BTreeMap<String, Chapter>`, at `*.body`: entry names match `^[A-Za-z]{2,3}(?:[-_][A-Za-z]{4})?(?:[-_](?:[A-Za-z]{2}\|[0-9]{3}))?$`, at `*.body.*`: matches `[^\\t\\n\\u000B\\f\\r \\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000]`, at `*.title`: must contain: `en` | `BOOK_CHAPTERS` | `{ intro = { body = { en = Start here. }, title = { en = Introduction } } }` | — | Chapters, by slug. |
