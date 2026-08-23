# Compared with other crates

The five crates nearest to this one, and what each of them lacks.

| Crate | What it gives you | What it lacks |
|-------|-------------------|---------------|
| [`figment`](https://crates.io/crates/figment) | the layering engine this builds on | no secrets-directory provider, no reload |
| [`figment_file_provider_adapter`](https://github.com/nitnelave/figment_file_provider_adapter) | the `_FILE` suffix half | no secrets directory; resolves a doubly-supplied key by precedence; last released October 2023 |
| [`confique`](https://github.com/LukasKalbertodt/confique) | a well-maintained figment alternative | no reload |
| [`settings_loader`](https://github.com/dmrolfs/settings-loader-rs) | almost this precedence order, and `.with_secrets(path)` | hot reload is a listed future enhancement, not a feature |
| [`hot_reload`](https://github.com/junkurihara/rust-hot-reloader) | the closest single match | requires `V: Eq + PartialEq`, which a config holding a `SecretString` cannot satisfy; its file reloader watches files non-recursively, which misses a volume remount |
