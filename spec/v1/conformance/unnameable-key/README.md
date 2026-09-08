# `unnameable-key`

A key the environment cannot name, and the field that says which kind of "cannot".

This is the [tier 2](../../CONFORMANCE.md#tier-2--dialect) case. Deriving a variable name from a key
path is the easy half of dialect conformance; agreeing about the paths where no derivation is
possible is the half that costs a decision, and an implementation that hands naming to a binder with
its own relaxed-binding rules will disagree here first.

## What it pins

`camelCase` key paths under an environment layer that folds names to lower case on the way in. The
fold is not reversible, so no variable name maps back to `distDir` — the derived
`EDGE_DIST_DIR` arrives as the key `dist_dir`, which is a *different key* and does not exist.

Both keys therefore carry:

```json
{ "env": null, "env_file": null, "secrets_file": null, "unreachable": "unnameable" }
```

The keys are real. The document layer supplies them perfectly well, `required` and `default` mean
what they always mean, and the `json_schema` half lists them like any other property. What is absent
is every *environment* spelling.

## Why the reason is published rather than inferred

A bare `null` would have two meanings and a consumer would have to guess:

- **`unnameable`** — nothing names the key, so the document is the only layer left. A consumer
  telling an operator "set `EDGE_DIST_DIR`" would be sending them to set a variable that quietly
  fills a different key.
- **`indirection`** — a name this schema gives to *another* key also fills this one. A producer
  refuses to build a contract containing it, so it reaches a consumer only in a schema rendered for
  documentation — but a consumer that treated it as "skip this key" would pass a document where one
  variable silently supplies two keys.

A publishing implementation MUST NOT emit `env: null` without `unreachable`. The meta-schema
enforces it as a conditional; `tests/spec.rs` asserts it against this case.

## The trap

A key with a working **alias** is not unnameable, even when its canonical path is. `unnameable` is
set only when *nothing* names the key — canonical spelling and every alias. Setting it from the
canonical spelling alone would tell an operator that a working configuration is impossible, which is
worse than the bare `null` it replaced. A `rename_all` container carrying one compatibility alias is
exactly that shape, and it is the case to test an implementation against after this one.

`indirection` is the opposite quantifier: it wins wherever it appears, even beside a spelling that
works, because it is not "this key is out of reach" but "some name in this schema also fills it".

## Source

Reference implementation — `tests/spec.rs`:

```rust
#[derive(Deserialize, Serialize, Describe)]
#[serde(rename_all = "camelCase")]
struct Unnameable {
    /// Where the bundle is read from.
    #[serde(default = "default_dist")]
    dist_dir: String,
    /// How long a rendered page stays fresh.
    #[serde(default)]
    max_age_secs: u64,
}

Terrace::new("EDGE_")
    .schema::<Unnameable>()
    .with_defaults_from(&Unnameable::default())?
    .into_contract(App::new("edge").version("1.0.0"))
    .build()?
```
