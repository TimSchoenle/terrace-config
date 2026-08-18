# The config contract: image-embedded configuration schemas, validated by the charts

A design for shipping each service's configuration surface *with its image*, so that
[helm-charts](https://github.com/TimSchoenle/helm-charts) can prove — before anything reaches a
cluster — that the `config.toml` it renders is one the pinned image will actually accept.

Three repositories are involved:

| Repository | Role |
|---|---|
| `TimSchoenle/terrace-config` | produces the contract document from the Rust types |
| `TimSchoenle/Portfolio` (and every other service) | emits it at build time and attaches it to the image |
| `TimSchoenle/helm-charts` | vendors it, and gates every chart render against it |

---

## 1. The defect this closes

`charts/portfolio` renders a `config.toml` into a ConfigMap from its own values:

```gotemplate
{{- define "portfolio.derivedConfig" -}}
isr:
  cache_dir: {{ .Values.isr.cacheDir | quote }}
  ttl_secs: {{ .Values.isr.ttlSecs }}
{{- end -}}
```

Nothing anywhere checks that `isr.ttl_secs` is still a key the server reads. `serde` ignores an
unknown key by design, so the day the app renames it:

- `helm template` renders.
- `values.schema.json` passes — it describes the *chart's* values, not the app's config.
- `kubeconform` passes — it validates Kubernetes objects, and a ConfigMap holding arbitrary text
  is a valid ConfigMap.
- `helm lint`, `kube-linter`, `helm unittest`, the e2e install: all pass.
- The pod starts, silently on the compiled default, and the ISR cache never revalidates.

The same hole exists on three other surfaces the chart writes:

- **Environment.** `portfolio.env` emits `PORTFOLIO_ISR__CACHE_DIR` specifically to beat the value
  baked into the image. Rename the key and that variable becomes a no-op — and worse, the comment
  explaining why it exists stays true-looking.
- **Secrets file names.** A key-named file in `$PORTFOLIO_SECRETS_DIR` is spelled `github__token`.
  A rename breaks the mount with no error at either end.
- **Collisions.** A key supplied by *both* the environment and the secrets directory is a boot
  failure under the default `ShadowPolicy::Reject`. A chart can construct that pair today and only
  find out from a CrashLoopBackOff.

Every one of these is knowable at render time, because `terrace-config`'s `schema` feature already
computes all of it from the types. The only thing missing is a way to get that knowledge from the
image the chart pins to the CI job that renders the chart.

**The mechanism is a digest-addressed document attached to the image.** Everything below is how it
is produced, where it lives, and what is checked with it.

---

## 2. What is produced

### 2.1 One envelope, not four files

`Schema` already renders four ways. Two of them matter to a machine:

| Rendering | Answers |
|---|---|
| `to_json()` | every key with its `path`, `env`, `env_file`, `secrets_file`, `ty`, `required`, `secret`, `reserved`, `aliases`, plus `dialect` and `loader` |
| `to_json_schema_with(DRAFT_07, closed)` | is *this TOML document* valid |

Neither subsumes the other. The JSON Schema cannot validate an environment variable name or a
secrets file name; the contract cannot be handed to a stock JSON Schema validator. Publishing them
as two artifacts means two hashes, two fetches and two chances to be half-stale, so they ship as
one document:

```json
{
  "terrace_contract": 1,
  "app": {
    "name": "portfolio",
    "version": "v2.5.0",
    "revision": "b47ca70…",
    "created": "2026-08-18T09:12:44Z"
  },
  "schema": { "schema_version": 1, "dialect": {…}, "loader": […], "keys": […] },
  "json_schema": { "$schema": "http://json-schema.org/draft-07/schema#", … },
  "external": { "env": […], "ignore": ["KUBERNETES_*"], "unknown": "reject" }
}
```

`terrace_contract` is the envelope's own version, independent of the `schema_version` inside it.
A consumer that does not recognise it refuses the document by name instead of misreading it. Every
field is `snake_case`, the envelope's own included — one document in two conventions is a field
name a consumer gets right from memory under one and guesses at under two.

**Status: implemented**, and revised after a full Phase 0 + Phase 1 implementation against
`TimSchoenle/Portfolio` found four blockers in the design below. See §2.4 for `external`, the piece
that turns this from a terrace-only gate into a whole-image one, and §2.5 for the constraints that
make it readable by something that is not Rust.

**No `app.digest`.** It was in this document and it cannot work: a digest is what building the image
*produces*, so a field carrying it must be written after the push — changing the bytes
`dev.terrace.config.contract.sha256` was computed over before it, which §3.3 defines as a hard
error. Absent, §12.3 would have nothing to compare. There is no build order satisfying both. The
field is also unnecessary: a registry artifact's subject *is* a digest, so a consumer that fetched
the document by asking a digest for its referrers already knows which image it belongs to. **The
attachment is the tie.** §12.3 is rewritten accordingly.

### 2.2 Determinism is a requirement, not a nicety

The document is hashed, committed, and diffed. It must be byte-identical across runs:

- The generator reads nothing from the environment — already guaranteed, and already the stated
  design of `examples/config-schema.rs`.
- Key order is declaration order; `serde_json::Map` is ordered whichever feature set is on.
- `app.created` and `app.revision` are the only fields that move between builds of the same source.
  **They live in `app`, never inside `schema` or `json_schema`** — so the chart repo can diff the
  half that describes the configuration and ignore the half that describes the build.

Add a test in `terrace-config` asserting `to_contract` is stable across two calls, and one
asserting a round trip through `serde` preserves it.

### 2.3 A reverse gate in the app repo

The service repo checks in its own copy and fails on drift, exactly as `Portfolio` already does for
`config.example.toml`:

```yaml
- run: cargo run -p portfolio-config --features config-schema --example config-schema -- --format contract > docs/config.contract.json
- run: git diff --exit-code -- docs/config.contract.json
```

This is what makes a configuration change *visible in the pull request that makes it*. A removed
key showing up in a diff is the cheapest possible signal that a chart is about to break, and it
costs one CI step.

**Do not pass `--revision` or `--created` here.** The container build does, and should; this gate
must not, or every commit fails it on two fields that are supposed to move. The committed document
is deliberately not byte-identical to the published one — it is the half that describes the
configuration, which is the half worth diffing.

### 2.4 `external` — the surface no derive can see

A service reads variables that are not its configuration. `PORT`, `IP` and `RUST_LOG` belong to
the Dioxus toolchain, which reads them before any of these layers exist; a base image contributes
`PATH` and `SSL_CERT_FILE`. None carry the loader's prefix, so no `Describe` implementation can
report them — and a gate that flagged everything it could not account for would flag all of them.

`External` is a *positive* declaration, not a suppression list:

| | What it says | What the gate does |
|---|---|---|
| `External::var` | this image reads it, and here is its type | checks it exactly like a configuration key |
| `External::ignore` | nobody here owns it | skips it; only a trailing `*` is a wildcard |
| `External::unknown` | what to do with everything else | `Reject` by default |

The distinction is the whole value. A declared `PORT` with `ty("u16")` means a chart passing
`PORT: "http"` fails the same gate a chart passing `PORTFOLIO_ISR__TTL_SECS: "soon"` fails. An
*ignored* `PORT` is a variable the chart may misspell freely. So the chart's `server.port`,
`server.host` and `logLevel` — which today are typed by nothing anywhere — become checked values,
and the pipeline covers every variable a pod carries rather than only the prefixed ones.

`build()` refuses five things outright, each a way a contract could quietly stop being one:

- an external variable **carrying the loader's prefix**: everything in that namespace is a
  configuration key, and declaring one external would leave it governed and exempt at once, with
  the exemption winning.
- an **ignore pattern reaching into that namespace** — the same exemption through the other door,
  and worse, because a pattern exempts everything it happens to cover rather than one named
  variable. `ignore("PORTFOLIO_*")` is the obvious spelling; the dangerous one is `ignore("PORT*")`,
  which carries no prefix, reads as a pattern about the external `PORT`, subsumes `PORTFOLIO_`
  entirely, and looks correct in review. An *exact* `ignore("PORT")` is fine — it matches that name
  and nothing else, and no key is spelled that. Together with the first, these are the
  security-relevant refusals: they are the only ways an application could remove a real key from
  the gate that owns it.
- an external variable **colliding with a spelling the loader reads**, the same defect reached
  through a `reserve`d name or a renamed prefix;
- an **ignore pattern covering a spelling the loader reads**, which is that defect through the
  suppression list. The prefix is not the whole namespace: a key's environment spelling is derived
  from it, but `config_var`, `secrets_dir_var` and `reserve` take arbitrary names, so
  `ignore("CREDENTIALS_*")` against `secrets_dir_var("CREDENTIALS_DIR")` exempts the variable that
  decides where every credential is read from — worse than exempting a key, because it loses all of
  them at once;
- an external variable **declared twice**, on `Schema::merge`'s reasoning;
- a **secret carrying a default**, anywhere in the document, checked at the boundary the document
  crosses into a public registry rather than trusted to the code paths that build it.

### 2.5 Constraints, so the document is readable by something that is not Rust

`ty` is a Rust type name and there is no published vocabulary for one. A consumer given only that
writes a mapping table — `bool`, `u8`…`usize`, `String`, `PathBuf`, `IpAddr`, plus a regex to unwrap
`Option<T>` — by reading the service's source, in whatever language it is written in, once per
consumer. `PathBuf` is the trap: it is a string and nothing in the name says so.

So every key and every declared external variable carries its constraints as JSON Schema keywords —
**two of them, because a value exists in two forms and a validator meets both**:

```json
{ "path": "isr.ttl_secs", "env": "PORTFOLIO_ISR__TTL_SECS", "ty": "u64",
  "constraint":      { "type": "integer", "minimum": 0 },
  "text_constraint": { "type": "string", "pattern": "^\s*\+?[0-9]+\s*$" } }
```

In a TOML file `ttl_secs = 0` is an integer. In the environment it is the characters `"0"`, and
`"0"` fails `{"type": "integer"}` under every conforming validator — so a document carrying only
the first would have gate 2 either rejecting correct deployments or silently coercing by a rule
nobody wrote down. That is the §2.4 ordering failure one level down, and it is what a first
implementation of this section actually hit.

`json_schema` carries `constraint` again, nested, at the key's position in the document. The flat
copies are for the gates that check the **environment**: gate 2 has a variable name and a string,
not a document, and `external.env` has no nested schema at all. `PORT: "http"` is catchable because
of `text_constraint` and not because of `constraint`.

The text patterns are **measured against the loader**, not derived from TOML's grammar, because
figment's `Env` provider decides them and its parse is neither TOML's nor `str::parse`'s. For a
`u64` it takes `0`, `42`, `007`, `+5` and `7` with surrounding whitespace and refuses `1_000`,
`0x1F`, `0b1`, `1e3`; for a `bool` it takes `true` and `false` and nothing else. The emitted pattern
is a superset of what was measured, because a pattern rejecting text the loader accepts stops a
deployment that was correct.

Each key also carries a **`text_form`** — `text`, `integer`, `boolean`, `choice`, `structured` or
`unknown` — which is what a consumer reads to choose the parse for the range step. Inferring the
parse from the constraint's shape ("a pattern means integer") was right while the producer emitted
two shapes and wrong the moment it emitted a third.

It also gives `text_constraint: null` one meaning rather than two. `text` means any text is fine;
`unknown` means nothing could be determined. Those were indistinguishable in the first
implementation, and a list-typed key is what made the difference cost a deployment: a `Vec<T>` needs
a bracketed TOML literal, so `PORTFOLIO_GITHUB__REPOS=a,b` — the first thing anyone would try — was
refused by the loader and passed every gate. `structured` keys now carry a pattern requiring the
bracket form.

`ExternalVar::constraint` states both constraints by hand for a type the crate cannot interpret,
`ExternalVar::text_form` states the form beside them, and the derive leaves what it finds alone.

**Gate 3 gets a blunter rule than a pattern.** The secrets directory and `_FILE` targets deliver
their contents as strings with no parse, and `Figment::extract` does not coerce a string into a
number or a boolean — so a key whose `constraint` is anything but a string type *cannot be supplied
by either*, whatever the file holds. Not "must match a pattern": cannot be supplied. A chart
mounting `isr__ttl_secs` as a secret file has made a mistake no file contents can fix, and the gate
can say so from `constraint` alone.

### 2.6 What the contract cannot say: platform-injected variables

Kubernetes service links inject `<SERVICE_NAME>_SERVICE_HOST`, `<SERVICE_NAME>_PORT` and five more
per Service in the namespace. The service name is the **release** name, so for a release called
`portfolio` they land inside a `PORTFOLIO_` prefix and gate 2 reports five failures on a correct
deployment; for a release called `staging-portfolio` they fall outside it entirely.

There is deliberately no API for declaring them. An image cannot know the release names it will be
deployed under, so no declaration written at build time is right for both cases. It belongs to
whatever renders the deployment, which does know:

**`enableServiceLinks: false` is a precondition of adopting this gate**, and every chart in §4 sets
it. It is not merely a validation nuisance — `PORTFOLIO_PORT` is a spelling of the key `port`, so
with service links on, a Service named after the release *supplies* that key from the environment
layer, outranking the mounted file. That is a live misconfiguration the contract cannot fix and
will not hide.

`HOSTNAME` and `KUBERNETES_*` do not carry the prefix and are ordinary `external.ignore` entries.

---

## 3. Where it lands on the image

Three carriers, one hash. They are not redundancies — each answers a different question, and the
shared `sha256` is what turns three copies into one consistency guarantee.

### 3.1 Labels — discovery

In the image config blob, so `crane config` or `docker buildx imagetools inspect` reveals them in
one request with no layer pull:

```dockerfile
LABEL dev.terrace.config.contract.version="1" \
      dev.terrace.config.contract.path="/config/contract.json" \
      dev.terrace.config.contract.sha256="${CONTRACT_SHA256}" \
      dev.terrace.config.prefix="PORTFOLIO_"
```

This is the *protocol*. "Does this image declare a config contract, and what should it hash to" is
answerable for any image in any registry without knowing anything about the project. It is also
what makes the scheme language-agnostic: a Go service that emits the same envelope and sets the
same four labels participates fully, and `terrace-config` is simply what makes it free for a Rust
service.

### 3.2 A file in the image — the literal ask, and the offline copy

```dockerfile
COPY --from=contract-builder /out/contract.json /config/contract.json
```

About 20–40 KB in a `scratch` image that is otherwise one static binary and an asset tree. What it
buys:

- The image is self-describing with no registry at all: `crane export`, `docker save`, an air-gapped
  mirror, a tarball on disk.
- It is the fallback path when a registry does not implement the referrers API.
- It is what a future initContainer or admission webhook reads to do the same validation
  *in-cluster* rather than in CI (§7).

It does **not** require the binary to grow a `--dump-schema` flag, which matters: the image is
`FROM scratch` with no shell, and the `schema` feature is deliberately kept out of the production
build. A generated file COPYed from a builder stage costs the runtime nothing.

### 3.3 An OCI referrer — the canonical fetch

```bash
oras attach --artifact-type application/vnd.terrace.config-schema.v1+json \
  "ghcr.io/timschoenle/portfolio@${DIGEST}" contract.json:application/json
cosign sign --yes "$(oras discover -o json --artifact-type … "…@${DIGEST}" | jq -r …)"
```

This is what the chart repo actually fetches, for four reasons:

1. **It is attached to the digest the chart pins.** `image.tag:
   v2.5.0@sha256:48e259cb…` — the referrer subject is that exact digest, so the contract cannot be
   for a different build than the one being deployed. A tag can be moved; this cannot.
2. **The image is untouched.** No extra layer, no digest churn, so the existing digest-pinned
   reproducible-build story is preserved — and a contract can even be attached *after* the fact to
   an image already released.
3. **Two small HTTP calls.** No layer pull. This matters at eight components per chart.
4. **It is signable.** `cosign sign` on the referrer, verified keyless against the app repo's
   workflow identity when the chart repo refreshes.

Fallback: OCI 1.0 registries without referrers get the `sha256-<digest>.<suffix>` tag scheme, which
`oras` handles transparently.

**Consistency check.** Whatever the chart repo fetches, it verifies `sha256(document) ==
dev.terrace.config.contract.sha256`. A referrer that disagrees with the label is a hard error, not
a fallback — the two disagreeing means one of them is from a different build.

### 3.4 Build wiring in `Portfolio`

A new stage, on the existing builder image so no toolchain is added, and a `COPY` into `runtime`:

```dockerfile
FROM builder AS contract-builder
RUN cargo run -p portfolio-config --features config-schema --example config-schema \
      -- --format contract > /out/contract.json

FROM scratch AS runtime
COPY --from=contract-builder /out/contract.json /config/contract.json
```

**The labels come from the generator, not from `LABEL` instructions.** That was wrong in the first
draft of this document: `Contract::labels` exists so that a Dockerfile never spells a label name by
hand — the failure mode of a typo being a contract that is silently never found — and a `LABEL`
key cannot be read from a build, so the Dockerfile spells all four by hand anyway and `labels()`
earns nothing.

```bash
cargo run -p portfolio-config --features config-schema --example config-schema \
  -- --format contract > contract.json
sha256="$(sha256sum contract.json | cut -d' ' -f1)"

args=()
while IFS= read -r label; do args+=(--label "$label"); done < <(
  cargo run -p portfolio-config --features config-schema --example config-schema -- --format labels
)
args+=(--label "dev.terrace.config.contract.sha256=${sha256}")

docker buildx build "${args[@]}" .
```

A repository that would rather keep `LABEL` instructions in the Dockerfile can, provided CI diffs
them against `--format labels` — so a typo fails a build rather than a deployment. What is not an
option is hand-written labels with nothing checking them.

The `oras attach` and `cosign sign` steps run after the push, keyed on the digest
`docker/build-push-action` returns. The document is attached **verbatim** — the bytes that were
hashed into the label, with nothing added afterwards. See §2.1 for why there is no digest field to
add.

---

## 4. The chart repo side

### 4.1 A per-chart contract declaration

`charts/<chart>/config-contract.yaml` — self-describing, so adding a chart or a component never
touches a central file. This mirrors the repo's own principle in `just chart-index`: "every value
comes from a chart's own `Chart.yaml`, so the table cannot drift".

```yaml
# What this chart's rendered configuration must satisfy, and which images decide that.
documents:
  - name: server

    # Where the rendered document is, in the output of `helm template`.
    source:
      kind: ConfigMap
      selector: { app.kubernetes.io/component: server }   # matched by label, not by name
      key: config.toml
      format: toml

    # Every binary that reads this document. Their contracts are unioned (§4.3).
    images:
      - values: image          # the values path `common.image` resolves

    # The pods that mount it, for the environment and secrets checks (§5.2, §5.3).
    consumers:
      - workload: { kind: Deployment, selector: { app.kubernetes.io/component: server } }
        containers: [portfolio]

    # Pairs this document is not checked for, each with a reason.
    exempt:
      - values: ci/extra-toml-values.yaml
        gates: [closed]
        reason: >-
          configExtraToml is appended verbatim and never parsed by the chart, so keys it
          introduces are invisible to the renderer that would have to declare them.
```

For `tankovault`, `documents:` gains one entry per rendered ConfigMap and each lists the images
that read it. `charts/teamspeak/config-contract.yaml` is:

```yaml
documents: []   # upstream image; no terrace contract to check against
```

That explicit opt-out is deliberate. A chart with a first-party image and *no* file is caught by
`just check-contract-coverage`, so the gate cannot be escaped by forgetting.

### 4.2 Vendored contracts

`charts/<chart>/contracts/<component>.json`, committed, refreshed by CI.

Vendoring rather than fetching on every run, for three reasons in ascending order of importance:

1. **Offline and fast.** The repo already carries a three-attempt retry loop around `helm template`
   because `values.schema.json` resolves Kubernetes `$ref`s over the network. Adding a second
   networked dependency to the hot path of every render would compound that.
2. **Reproducible.** Re-running CI on a six-month-old commit validates against what was true then,
   not against whatever the registry serves today.
3. **Reviewable — this is the real reason.** The contract diff lands in the *same pull request* as
   the digest bump:

   ```diff
   -  "path": "isr.ttl_secs",
   -  "env": "PORTFOLIO_ISR__TTL_SECS",
   +  "path": "isr.revalidate_secs",
   +  "env": "PORTFOLIO_ISR__REVALIDATE_SECS",
   ```

   A human reviewing a Renovate digest bump sees the removed key next to the chart's failing gate.
   That is the moment the whole design exists to create.

Refresh belongs in the existing **Documentation** job, which already regenerates `values.schema.json`
and every README and commits the result back to the branch:

```
just contracts        # resolve each declared image to a digest, fetch, verify, write
just check-contracts  # `git diff --exit-code`, for anyone running it locally
```

`just contracts` verifies the cosign signature and the `sha256` label before writing. Once written
and committed, the file is trusted, and no gate below touches the network.

### 4.3 Unioning contracts — the non-obvious part

`tankovault` renders one root `config` merged with a per-service `config`, under one
`TANKOVAULT_` prefix, read by eight separate binaries. Each binary's `Describe` covers only the
keys it consumes; `serde` ignores the rest. So **validating one document against one binary's
schema with `additionalProperties: false` would reject a perfectly correct deployment** — every key
belonging to the other seven would be "unknown".

The correct object to validate against is the union of the schemas of every binary that reads the
document. `terrace-config` already names this exact problem and solves it in-crate with
`Schema::merge`, for workspaces with no single root type. Chart-side the same union is computed
structurally, over the `json_schema` halves:

| Situation | Rule | Why |
|---|---|---|
| a path in one schema only | keep | it belongs to that binary |
| a path in several, identical | keep once | the shared key both binaries read |
| `required` | union | a key any reader requires must be present |
| `additionalProperties` | `false` at every level | after the union, an unknown key is unknown to *all* of them |
| **any other keyword present in two schemas with different values** | **hard error** | two binaries disagree about one key |

The last row is a catch-all on purpose, and it is the row to get right. An earlier draft named
`type` and `enum` and left everything else to last-one-wins, which is a rule nobody wrote down: two
images disagreeing about a key's `maximum` is the same defect as disagreeing about its `type` —
one contract accepts a value the other refuses — and enumerating keywords means the next one added
to the producer falls through the gap silently. `minimum`, `maximum`, `items`, `uniqueItems`,
`default`, `description`, `$schema` and `$id` are all covered by saying it once.

`$schema` in particular: two contracts of one document declaring different dialects is a real
signal, not a formatting difference, and refusing it is what caught a producer bug where relaxing
one option silently moved a document off draft-07.

Same reasoning as `Schema::merge`'s: refusing to build is better than quietly picking one of two
descriptions. About sixty lines of Python, deterministic, unit-testable without a registry —
worth its own `just test-contract-union` over a fixture directory.

---

## 5. The gates

Ordered by cost. Gates 1–3 are offline, share one render, and run on every pull request.

### Gate 1 — the document

For each chart × each `ci/*.yaml`, reusing the existing `render-chart` so the manifests are
byte-identical to the ones `kubeconform` and `kube-linter` see:

1. render
2. select the object by `kind` + `selector`, take `data[key]`
3. parse TOML → JSON
4. validate against the union, `additionalProperties: false`

Catches: unknown key, wrong type, missing required key, value outside an enum, a table where a
scalar belongs.

### Gate 2 — the environment

**Per container, against that container's own image contract — not the union.** A container has one
image; a variable it carries that only a *sibling* image reads is the defect this gate is for.

Classification is the ordered list in §2.4's implementation — first match wins, and the step that
rejects an unaccounted-for prefixed variable sits *above* both external lists, so the two cannot
disagree about what is exempt. A value is checked twice: its text against `text_constraint`, then —
parsed the way `text_form` says — against `constraint`, which is the only step a `minimum` or
`maximum` is reached by. See §2.5.

Requires `enableServiceLinks: false` on every pod it checks (§2.6) — and the gate should *look* for
it. When step 4 rejects an unaccounted-for prefixed variable and the pod spec does not set it, the
message is not "this name is a mystery" but "this is a service link; set `enableServiceLinks:
false`". The contract names the fix, so the gate can too.

For every declared container, every variable matching the document's prefix must be one of:

- a key's `env` spelling,
- a key's `env_file` spelling,
- a `loader` variable (`PORTFOLIO_CONFIG`, `PORTFOLIO_SECRETS_DIR`),
- a `reserved` key.

Catches the `PORTFOLIO_ISR__CACHE_DIR` class directly: a variable the chart sets that the app no
longer reads is a silent no-op today and a failed render after this.

**And the collision the loader refuses.** The contract carries `env`, `env_file` and `secrets_file`
per key, so the validator can detect a key supplied by two of the last three layers — the exact
pair `ShadowPolicy::Reject` refuses at boot. That is a guaranteed CrashLoopBackOff turned into a
failed `helm template` naming both spellings. No other tool in the repository can see this, because
seeing it requires knowing that `PORTFOLIO_GITHUB__TOKEN` and the file `github__token` are the same
key.

### Gate 3 — file spellings

Per container, for gate 2's reason.

- every file name in a rendered Secret mounted at `secretsDir` must equal some key's `secrets_file`
- every `*_FILE` variable must equal some key's `env_file` and point inside a mounted volume
- and the key it names must have `text_form: text` — see §2.5. A key of any other form cannot be
  file-supplied at all, so a mount for one is a defect the file's contents cannot repair

Catches a `github.token` → `github.api_token` rename breaking a mount with no error at either end.

### Gate 4 — the real loader (not on every PR)

```bash
docker run --rm -v "$rendered:/etc/portfolio/config:ro" \
  -e PORTFOLIO_CONFIG=/etc/portfolio/config \
  "$image" --check-config
```

The binary loads the configuration with the real types, real `#[serde(default)]` functions, real
`ShadowPolicy` — and exits non-zero with the real error message. Zero fidelity gap: it catches
what no JSON Schema can, including a domain newtype's parse rules, a cross-field invariant, and a
`deny_unknown_fields`.

Cost is a container pull per component and an architecture match, so it belongs beside `test-e2e`
in the release and nightly workflows and behind a label on pull requests — not in the hot path.

Gates 1–3 are the fast approximation; gate 4 is the ground truth. Having both is what lets 1–3 stay
cheap without anyone having to argue about how faithful they are.

**Work in `terrace-config`:** a `Terrace::check()` that loads, prints the `explain` report and
returns a verdict — five lines for a service to wire into `main`, and it composes with `explain`,
which already exists and already prints which layer supplied every key. That output attached to a
failed CI job is a better diagnostic than anything a schema validator can produce.

---

## 6. Rollout

| Phase | Repo | Deliverable | Proves |
|---|---|---|---|
| 0 | terrace-config | `schema::Contract`, `Schema::to_contract`, `--format contract`, determinism tests | the document has one shape |
| 1 | Portfolio | contract stage, `COPY`, labels, `oras attach`, `cosign sign`, reverse drift gate | one image carries one contract |
| 2 | helm-charts | `config-contract.yaml` for portfolio, `just contracts`, `just check-config`, gates 1–3, a `config` CI job | the loop closes end to end on the simplest chart |
| 3 | helm-charts | tankovault: many documents, many images, the union | it scales past one binary |
| 4 | both | `--check-config`, gate 4 in e2e; `check-contract-coverage` | fidelity, and no silent opt-out |
| 5 | optional | cluster-side enforcement (§7) | the same artifacts, a second consumer |

Phase 2 is the one that must be got right; 3 is the one that will find the design errors. Phase 1
is deliberately a single-image chart so that the first end-to-end run has one moving part.

**Validate the loop before phase 1 ships:** hand-write a contract JSON for the current portfolio
image, commit it, and build gate 1 against it. If the gate does not fail on a deliberately renamed
key in `_helpers.tpl`, nothing downstream is worth building.

---

## 7. Extension points designed for, not built

- **Cluster-side admission.** The rendered ConfigMap carries
  `dev.terrace.config/contract-sha256`; a Kyverno policy fetches the same referrer for the running
  pod's image digest and refuses a mismatched pair. Same artifacts, no new generation — this is why
  the in-image copy (§3.2) is worth its 30 KB.
- **Non-TOML documents.** `source.format` is already a field. A YAML or JSON document normalises to
  the same tree, and every gate above is unchanged.
- **Non-Rust services.** The contract is defined by its media type and its four labels, not by
  `terrace-config`. Any language that emits the envelope participates.
- **Third-party images.** A `documents[].contract: { file: contracts/paperless.hand.json }` points
  at a hand-written contract for an upstream image, and every gate runs identically. The
  distinction is where the document comes from, never what is done with it.
- **The image's own `ENV` block.** A Dockerfile setting `ENV PORTFOLIO_ISR__CACHE_DIR=/tmp/isr`
  supplies that key on every run, and the contract reports the key with no default — so a chart
  asking what happens if it omits the key is told "ISR off" and gets "ISR on". `Key::default` is a
  claim about the *code*, and the image's defaults are a second surface no derive can see. Reading
  them is a `crane config` away and the union is a natural additive field; the gap is documented on
  `Key::default` in the meantime.
- **A drift report rather than a gate.** The same union, run across every published chart version
  against every published image digest, answers "which deployed releases are running on a config
  the image no longer reads" — a scheduled job, not a pull-request gate.

---

## 8. Risks, and what each is answered with

| Risk | Answer |
|---|---|
| registry outage breaks every PR | vendored contracts; only the refresh recipe touches the network |
| a compromised or swapped contract weakens the gate | cosign-verified at refresh, `sha256` cross-checked against the image label, committed and reviewed |
| a schema with a remote `$ref` makes CI depend on a third party | the validator refuses any `$ref` that is not `#/…` |
| the JSON Schema is more permissive than the real loader | acknowledged and bounded by design (`rust_type` emits nothing for a type it does not recognise); gate 4 is the answer, not tighter guessing |
| the JSON Schema is *stricter* than the loader — a false failure blocks a correct deploy | the crate's stated rule already: "a schema that rejects a file the loader would have accepted is worse than one that accepts a file the loader will reject" |
| an escape hatch (`configExtraToml`) is unvalidatable | declared per values-file in `exempt`, with a reason, and only that gate is relaxed |
| chart and app release out of order | everything keys off the pinned **digest**; the failure lands on the bump PR, which is the right place for it |
| a new chart quietly opts out | `check-contract-coverage`: a first-party image with no declaration fails |
| eight components make the gate slow | one render shared by all gates; contracts are local files; the network appears only in the docs job |

---

## 9. Summary

One document, generated from the Rust types by the crate that already knows them, attached to the
image digest the chart already pins, vendored into the chart repo by the job that already commits
generated files, and checked by a gate that already has a render to work from.

The result is that a configuration key renamed in a service becomes a **failing CI check on the
Renovate pull request that bumps that service's image digest**, with the old and new spellings side
by side in the same diff — instead of a pod that starts cleanly, reports healthy, and runs on a
default nobody chose.

---

# Part II — Wiring it into the helm-charts CI pipeline

The repository has one architectural rule that decides almost every question below:

> Every gate CI runs is a `just` recipe, and the workflows invoke those same recipes rather than
> keeping their own copy of the logic.

So nothing here is a workflow step that *does* something. The workflows gain one job that calls one
recipe, and the docs job gains one step that calls another. Everything else is `just/` and
`.github/scripts/`.

## 10. Job topology

Two touch points in `.github/workflows/ci.yaml`, and only one of them is new.

```
docs (existing, contents: write)
  └── + just contracts          # networked: fetch, verify, write the vendored contracts
                                #            → swept up by the existing commit-changes step

config-contracts (new, contents: read)
  └── just check-config         # offline: render, then gates 1-3

install (existing)
  └── + just test-config-live   # phase 4, networked, gated on lint.outputs.changed
```

`config-contracts` deliberately declares no `needs:`. The docs job already documents why nothing
gates on it — the follow-up push re-runs the whole workflow against the corrected tree, and gating
would drop the other checks on exactly the runs that change a chart. The staleness interlock in
§12.3 is what makes that safe here.

## 11. The recipes

A new `just/contracts.just`, imported from `justfile` alongside the other six groups.

```just
import 'just/contracts.just'
```

Three variables in `justfile`, beside `helm_unittest_version` and `kube_version`, for the same
reason those are there — a bump is a single edit rather than one per workflow:

```just
# Registry client and signature verifier for `just contracts`. Only the contract refresh needs
# these; every gate that reads a contract reads the committed file.
oras_version := "1.3.0"
cosign_version := "v2.6.1"

# JSON Schema engine for `just check-config`. A pinned binary rather than a pip install, so the
# recipe runs identically in a Git Bash shell and the scripts stay stdlib + PyYAML.
jv_version := "v0.7.0"

# The workflow identity a contract must be signed by to be vendored. Anything else is refused.
contract_signer := "https://github.com/TimSchoenle/[^/]+/.github/workflows/release.yml@refs/tags/.*"
```

### 11.1 The refresh — the only networked recipe

```just
# Refresh every vendored contract from the image each chart pins.
#
# The one recipe in this repository that talks to a registry. Everything downstream reads the
# committed file, so a registry outage cannot fail a pull request that changes no image.
#
# Run by the Documentation job, whose commit step carries the result back to the branch. Running
# it locally is a convenience; `just check-contracts` reports the gap without the network.
[doc("Refresh every vendored contract from the image each chart pins")]
[group('contracts')]
contracts chart='':
    #!/usr/bin/env bash
    set -euo pipefail
    {{ resolve_python }}
    CONTRACT_SIGNER='{{ contract_signer }}' \
      "$python" {{ scripts }}/refresh-contracts.py {{ chart }}

# Report vendored contracts that no longer match the images their charts pin.
[doc("Report vendored contracts that no longer match the images their charts pin")]
[group('contracts')]
check-contracts:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! git diff --exit-code -- '{{ charts }}/*/contracts/'; then
      echo "error: vendored contracts are stale; run 'just contracts'" >&2
      exit 1
    fi
```

`refresh-contracts.py`, per chart × per declared image:

1. read `charts/<chart>/config-contract.yaml`
2. resolve each `images[].values` path through `values.yaml` to `registry/repository:tag@digest`
3. `oras discover --artifact-type application/vnd.terrace.config-schema.v1+json <ref>@<digest>`
4. `cosign verify` against `$CONTRACT_SIGNER`, with the image digest as the subject
5. `crane config` → read `dev.terrace.config.contract.sha256`
6. `oras pull` → assert `sha256(document)` equals both that label and the digest the referrer
   descriptor claims
7. write `charts/<chart>/contracts/<component>.json`, with `app.digest` recorded inside

A failure at 4, 5 or 6 is fatal and names which of the three copies disagreed. There is no fallback
path to "fetch it unverified" — a contract that cannot be proven to belong to the pinned digest is
worse than none, because every gate downstream would trust it.

### 11.2 The gate — offline

```just
# Validate every chart's rendered configuration against the contracts of the images it pins.
#
# Reuses `just render`, so the manifests these gates read are byte-identical to the ones
# kubeconform and kube-linter see — one render, one set of facts, no third opinion about what the
# chart produces.
#
# Reads no network. The contracts are committed files, and the JSON Schema engine is refused any
# reference it would have to fetch.
[doc("Validate every chart's rendered configuration against its images' contracts")]
[group('contracts')]
check-config out='rendered':
    #!/usr/bin/env bash
    set -euo pipefail
    {{ resolve_python }}
    just render '{{ out }}'
    JV_BIN="$(command -v jv)" "$python" {{ scripts }}/check-config.py '{{ out }}'

# Fail on a chart that pins a first-party image and declares no contract.
[doc("Fail on a chart that pins a first-party image and declares no contract")]
[group('contracts')]
check-contract-coverage:
    #!/usr/bin/env bash
    set -euo pipefail
    {{ resolve_python }}
    "$python" {{ scripts }}/check-config.py --coverage-only

# Unit tests for the contract union, over fixtures rather than over a registry.
[doc("Unit tests for the contract union")]
[group('contracts')]
test-contract-union:
    #!/usr/bin/env bash
    set -euo pipefail
    {{ resolve_python }}
    "$python" -m unittest discover -s {{ scripts }}/tests -p 'test_contract_*.py' -v
```

And the aggregate gains them, so a local shell reproduces CI with one command as it does today:

```just
check: deps test validate-manifests check-immutable check-config check-contract-coverage lint lint-policy
```

`contracts` is deliberately *not* in `check`. `check` is the cluster-free set, and it is also the
network-free set once this lands — the only reason a contributor's `just check` should fail on
connectivity is the Kubernetes reference resolution that already exists.

### 11.3 Why a pinned `jv` and not `pip install jsonschema`

The scripts in `.github/scripts/` are stdlib + PyYAML, and `resolve_python` exists because this
repository is developed from a Git Bash shell on Windows, where a `pip install` inside a recipe is
the difference between a gate that runs locally and one that does not. A pinned single binary,
installed exactly as `kubeconform` already is, keeps that invariant:

```yaml
- name: Install jv
  env:
    JV_VERSION: v0.7.0
  run: |
    set -euo pipefail
    curl -fsSL "https://github.com/santhosh-tekuri/jsonschema/releases/download/${JV_VERSION}/jv-linux-amd64.tar.gz" \
      | sudo tar -xz -C /usr/local/bin jv
    jv --version
```

TOML parsing needs nothing extra: `tomllib` is stdlib.

## 12. What `check-config.py` does

One pass, every violation collected before it exits — the repository's existing posture, stated in
`just render` ("every pair is attempted before the recipe exits, so one chart that fails to render
does not hide the state of the rest") and in `validate-manifests` ("kubeconform validates every
file before it exits").

### 12.1 The loop

```
for chart in charts/*/:
    contract = charts/<chart>/config-contract.yaml     # absent → skip; documents: [] → opt-out
    for document in contract.documents:
        union = merge(vendored contracts of document.images)      # section 4.3
        for rendered in rendered/<chart>--*.yaml:
            if (values file, gate) in document.exempt: relax that gate only

            # One document, read by every image — so the union, or a key belonging to one
            # binary is "unknown" to the schema of another.
            gate 1  the ConfigMap's data[key], TOML to JSON, against union.json_schema

            # One container, one image — so that container's own contract, never the union.
            for consumer in document.consumers:
                gate 2  every env var on the container, against consumer's contract
                gate 3  its secret file names and _FILE targets, likewise
```

**The scopes are different and the difference is the point.** Gate 1 is about a file every binary
reads. Gates 2 and 3 are about one container, which has exactly one image, so checking them against
the union reintroduces precisely what splitting the scopes removed — a variable set on the
`update-repos` container that only the server reads passes against the union and is exactly the
defect gate 2 exists to catch. Use the union where the artefact is shared and the image's own
contract where it is not.

### 12.2 Output

One line per violation, prefixed by the pair that produced it, so a failure names the chart, the
values file and the key:

```
portfolio--default-values.yaml: server: config.toml: isr.ttl_secs: no such key
  (did you mean isr.revalidate_secs? renamed in portfolio v2.6.0)
portfolio--default-values.yaml: server: env: PORTFOLIO_ISR__TTL_SECS set by container
  "portfolio" matches no key in the contract
tankovault--full-values.yaml: api: auth.session_ttl: type mismatch: schema says integer,
  the chart renders "3600"
```

The "did you mean" comes free: the union already holds every key path, so a Levenshtein pass over
it costs nothing and turns a rename from a puzzle into a one-line answer. Violations also go to
`$GITHUB_STEP_SUMMARY` as a table, which is where a reviewer of a Renovate bump will look first.

### 12.3 The staleness interlock — the important part

Because the docs job does not gate the others, a Renovate digest bump produces this sequence:

1. **run 1** — the tree has a new digest and the *old* contract. The docs job refreshes and
   commits; the config job runs concurrently, against the old contract.
2. the commit re-triggers the workflow.
3. **run 2** — new digest, new contract. The config gate is authoritative.

Run 1's config job must never report a pass it cannot justify. So it does not validate at all
unless the contract provably belongs to the image.

The digest is **not** in the contract — see §2.1 — so the chart repo records it on the way in. The
published document is vendored inside a wrapper `just contracts` writes, keeping the published
bytes exactly the ones that were hashed:

```json
{
  "source": {
    "image": "ghcr.io/timschoenle/portfolio",
    "digest": "sha256:48e259cb…",
    "sha256": "3f1c…",
    "fetched": "2026-08-18T09:12:44Z"
  },
  "contract": { "terrace_contract": 1, … }
}
```

`sha256` is over `contract` as published, so the vendored copy can still be checked against the
image label without re-fetching. The interlock then reads:

```
if vendored["source"]["digest"] != resolve_digest(values, document.images[i]):
    fail: "charts/portfolio/contracts/server.json is for sha256:48e259cb..., but the chart
           pins sha256:9a1f22e7.... The Documentation job refreshes it; re-run after its
           commit, or run 'just contracts' locally."
```

The provenance a chart repo needs is chart-repo-shaped, which is why it lives in the chart repo's
wrapper rather than in a document a hundred other consumers also read.

Deterministic, offline, self-healing, and a hard failure rather than a skip — the whole design
turns on the gate being trustworthy on exactly the pull request that bumps a digest.

### 12.4 No remote references

Before `jv` is invoked, the union is walked and any `$ref` not beginning with `#` is a fatal error.
The chart `values.schema.json` files legitimately reference Kubernetes types by URL, which is why
`render-chart` carries a three-attempt retry; the *app config* schema must not, or an offline gate
silently becomes a networked one and a third party gains a say in what CI accepts.

## 13. The workflow diffs

### 13.1 `docs` — one step

Between `Generate schemas` and `Run helm-docs`, so the existing `commit-changes` step sweeps the
result up with everything else it already commits:

```yaml
      - name: Install oras and cosign
        env:
          ORAS_VERSION: '1.3.0'
          COSIGN_VERSION: 'v2.6.1'
        run: |
          set -euo pipefail
          curl -fsSL "https://github.com/oras-project/oras/releases/download/v${ORAS_VERSION}/oras_${ORAS_VERSION}_linux_amd64.tar.gz" \
            | sudo tar -xz -C /usr/local/bin oras
          curl -fsSL -o /tmp/cosign "https://github.com/sigstore/cosign/releases/download/${COSIGN_VERSION}/cosign-linux-amd64"
          sudo install -m 0755 /tmp/cosign /usr/local/bin/cosign

      - name: Refresh image configuration contracts
        run: just contracts
```

The commit message the existing step uses — `docs: update Helm chart documentation and schemas` —
already covers it, and the PR comment step needs no change.

If any image moves to a private repository, this is the one job that needs `packages: read` and a
registry login. Nothing else in the pipeline talks to a registry.

### 13.2 `config-contracts` — the new job

```yaml
  # The rendered configuration is the one artifact in this repository that no other gate can see
  # into: `values.schema.json` describes the chart's values, kubeconform describes Kubernetes
  # objects, and a ConfigMap holding a stale `config.toml` is a valid ConfigMap by both. This job
  # validates that document against the contract published by the image the chart pins — and the
  # environment variables and secret file names beside it, which are the same contract's other
  # half. Offline: the contracts are committed files.
  config-contracts:
    name: Configuration Contracts
    runs-on: ubuntu-latest
    permissions:
      contents: read # to checkout code
    steps:
      - name: Harden Runner
        uses: step-security/harden-runner@... # v2.20.1
        with:
          egress-policy: audit

      - name: Checkout
        uses: actions/checkout@... # v7
        with:
          persist-credentials: false

      - name: Setup toolchain
        uses: ./.github/actions/setup-toolchain

      - name: Build Helm dependencies
        run: just deps

      - name: Install jv
        env:
          JV_VERSION: v0.7.0
        run: |
          set -euo pipefail
          curl -fsSL "https://github.com/santhosh-tekuri/jsonschema/releases/download/${JV_VERSION}/jv-linux-amd64.tar.gz" \
            | sudo tar -xz -C /usr/local/bin jv
          jv --version

      - name: Validate rendered configuration against image contracts
        run: just check-config

      - name: Check every first-party image declares a contract
        run: just check-contract-coverage

      - name: Contract union unit tests
        run: just test-contract-union
```

It mirrors `policy-scan` almost exactly — same permissions, same rendering, one pinned binary — so
it needs no new reviewer intuition.

### 13.3 `install` — phase 4, later

Gate 4 is a container pull per component and an architecture match, so it goes beside the e2e
install rather than in the hot path, behind the same `needs.lint.outputs.changed` condition:

```yaml
      - name: Load the pinned configuration with the real binary
        if: needs.lint.outputs.changed == 'true'
        run: just test-config-live
```

`test-config-live` renders, writes each document to a temp dir, and runs the pinned image with
`--check-config`. Its failure output is the service's own `explain` report — which layer supplied
every key — and that is a better diagnostic than any schema validator can produce.

## 14. Rolling it out without breaking the pipeline

The gate is worthless if it is merged red and disabled a week later. Four steps, each mergeable:

1. **Recipes and scripts, no job.** `just check-config` exists and passes locally against a
   hand-written contract for the current portfolio digest. Nothing in CI changes.
2. **The job, non-blocking.** `config-contracts` added with `continue-on-error: true`, scoped to
   portfolio only. Two or three pull requests' worth of real runs, on real Renovate bumps, is what
   tells you whether the union rules and the exemptions are right.
3. **Blocking for portfolio.** Drop `continue-on-error`, add the job to branch protection. This is
   the point at which the gate is real for one chart.
4. **Widen.** tankovault next — many documents, many images, and the union — then everything else.
   `check-contract-coverage` moves from advisory to blocking last, once every chart has either a
   contract or a written opt-out.

The single deliberate falsification to run before step 1: rename `isr.ttl_secs` in
`charts/portfolio/templates/_helpers.tpl` and confirm `just check-config` fails, naming the key. If
it does not, no amount of CI wiring above it is worth building.
