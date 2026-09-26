| TOML | Type | Environment | Default | Flags | Purpose |
|---|---|---|---|---|---|
| `dist_dir` | `String` | `SITE_DIST_DIR` | `public` | — | Bundle directory the readiness probe checks. |
| `legal.documents` | `BTreeMap<String, LegalDocument>`, must contain: `imprint`, `privacy`, entry names match `^[a-z0-9][a-z0-9_-]{0,63}$` | `SITE_LEGAL__DOCUMENTS` | — | required | Legal documents, by the name the site links them under. |
| `legal.links` | `BTreeMap<String, String>`, must contain: `home`, entry names match `^[a-z]+$` | `SITE_LEGAL__LINKS` | `{ home = / }` | — | Footer links, by label. |
