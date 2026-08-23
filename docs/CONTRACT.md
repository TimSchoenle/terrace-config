# Publishing the contract with the image

A contract is one document, attached to an image digest, saying what configuration the image takes.

The renderings in [SCHEMA.md](SCHEMA.md) describe a configuration to somebody who has the source.
A Helm chart's CI job does not: it holds an image digest and a `config.toml` it rendered itself,
and no way to know whether the two agree. So a chart renders `isr.ttl_secs`, the service renames
the key, `serde` ignores what it does not recognise, and the pod starts healthy on a compiled
default.

`Contract` is the document that closes that. One file, attached to one image digest, carrying both
machine-readable halves:

```rust
use terrace_config::Terrace;
use terrace_config::schema::{App, External, ExternalVar};

let contract = Terrace::new("PORTFOLIO_")
    .reserve("PORTFOLIO_PROFILE")
    .schema::<Config>()
    .with_defaults_from(&Config::default())?
    .into_contract(App::new("portfolio").version("v2.5.0"))
    .external(
        External::new()
            .var(
                ExternalVar::new("PORT")
                    .owner("dioxus")
                    .ty("u16")
                    .default("8080")
                    .docs("Bind port. Read by the Dioxus toolchain, not by this loader."),
            )
            .var(ExternalVar::new("RUST_LOG").owner("tracing").ty("String"))
            .ignore("KUBERNETES_*")
            .ignore("HOSTNAME"),
    )
    .build()?;

std::fs::write("contract.json", contract.to_json()?)?;
```

```json
{
  "terrace_contract": 1,
  "app": { "name": "portfolio", "version": "v2.5.0" },
  "schema": { "schema_version": 1, "dialect": { … }, "loader": [ … ], "keys": [ … ] },
  "json_schema": { "$schema": "http://json-schema.org/draft-07/schema#", … },
  "external": { "env": [ … ], "ignore": ["KUBERNETES_*", "HOSTNAME"], "unknown": "reject" }
}
```

Every field is `snake_case`, the envelope's own included. One document in two conventions —
`text_constraint` on a key and `textConstraint` on the variable beside it — is a field name a
consumer gets right from memory under one convention and guesses at under two.

Both halves, because neither is enough on its own. `json_schema` is the only one a stock JSON
Schema validator can act on, and it carries no environment spellings at all — so it cannot tell a
chart that the `PORTFOLIO_ISR__CACHE_DIR` it sets is no longer read, or that the file it mounts as
`github__token` is now named something else. `schema` carries every spelling and can be handed to
no validator. Published as two artefacts they would be two hashes and two chances to be half-stale.

The JSON Schema half defaults to draft-07 — the dialect Helm validates `values.schema.json`
against — and to `additionalProperties: false`, because an unknown key is the defect the document
exists to catch and an open schema catches none of them.

**It carries no `required` list**, and that default differs from `Schema::to_json_schema`'s for a
reason about meaning rather than strictness. JSON Schema's `required` says *this document must
carry the property*; `Key::required` says *some layer must supply the key*, and the loader takes the
environment or a mounted file just as readily. So a chart supplying a required **secret** from a
mount — the only way to supply a secret — renders a document a `required` list refuses and a
deployment that starts. A consumer checks `required` per key across every layer it can see instead,
which is where the evidence is, and can then say "no layer supplies this" rather than "add it to the
file". `ContractBuilder::require_present(true)` turns it back on; the standalone
`to_json_schema`, whose reader is an editor validating a hand-written file, is unchanged.

**The schema you pass is a claim about what *this image's binary* loads.** A workspace with several
aggregates has a generator that naturally reaches for the union of them, and a contract built from
that union asserts the runtime image reads a build-time credential no deployment supplies. Nothing
can check this: both schemas are well-formed and only you know which binary is in the image. Use
`Schema::merge` when several binaries really are.

## Every key says what its value must be — in both spaces

A configuration value exists in two forms and a validator meets both. In a TOML file
`ttl_secs = 0` is an integer; in the environment `PORTFOLIO_ISR__TTL_SECS=0` is the two characters
`"0"`, and `"0"` fails `{"type": "integer"}` under every conforming JSON Schema validator. So each
key carries two constraints, flat, beside the spellings:

```json
{ "path": "isr.ttl_secs", "env": "PORTFOLIO_ISR__TTL_SECS", "ty": "u64",
  "constraint":      { "type": "integer", "minimum": 0 },
  "text_constraint": { "type": "string", "pattern": "^\\s*\\+?[0-9]+\\s*$" },
  "text_form":       "integer" }
```

`constraint` describes the parsed value; `text_constraint` describes the characters an environment
variable holds before anything parses them. They are complementary, not alternatives — a consumer
checking a variable applies the second to the raw text and the first to whatever the parse
produced.

`json_schema` carries `constraint` again, nested, at the key's position in the document. The flat
copies are for the consumer that has a variable name and a string rather than a document. Without
them every consumer in every language reimplements a vocabulary of Rust type names by reading the
service's source — with `PathBuf` as the trap, since it is a string and nothing in the name says
so.

The text patterns were **measured against the loader**, not derived from TOML's grammar, because
figment's `Env` provider is what decides them and its parse is neither TOML's nor `str::parse`'s.
For a `u64` it takes `0`, `42`, `007`, `+5` and `7` with surrounding whitespace, and refuses
`1_000`, `0x1F`, `0b1` and `1e3`; for a `bool` it takes `true` and `false` and nothing else — not
`TRUE`, not `1`, not `yes`. The emitted pattern is a superset of what was measured, because a
pattern that rejects text the loader accepts stops a deployment that was correct.

**That provider also trims**, which is where the two spaces part company for a choice. `constraint`
for one is a bare `enum` — a TOML document must spell a variant exactly, and `level = "info "` in a
file really is refused. `text_constraint` is the same set with surrounding whitespace permitted,
because `PORTFOLIO_LOG_LEVEL="info "` loads. Trailing whitespace in a chart value is the ordinary
YAML footgun — a block scalar, a value interpolated from a file — so a copy of the bare enum would
refuse deployments that work. Booleans are the same shape for the same measured reason.

**`text_form` is what says how to read the text**, and it is always present: `text`, `integer`,
`boolean`, `choice`, `structured` or `unknown`. A consumer reads it to choose the parse for the
range step rather than inferring one from which keywords `text_constraint` happens to carry — an
inference that was right while there were two shapes and wrong the moment there were three.

It also gives `text_constraint: null` one meaning instead of two. `text` says any text is fine and
there is nothing to parse; `unknown` says nothing could be determined, which is a gap rather than
an answer. Those used to be indistinguishable, and a list-typed key is what made the difference
cost a deployment: a `structured` key — a `Vec<T>`, a map — needs a TOML literal, so
`PORTFOLIO_GITHUB__REPOS=a,b` is refused by the loader and used to pass every gate. Those now carry
a pattern requiring the bracket form.

A key's `env` can also be `null`, and `unreachable` says why — the two reasons differ in whether
the environment can reach the key at all. `unnameable` means no variable names it: a
`rename_all = "camelCase"` path never comes back through the case fold, and neither does one
carrying the nesting separator, so the document is the only layer left. `indirection` is the case
`build` refuses, and it is in the enum because a schema rendered for documentation still reports
it. A consumer meeting a bare `null` and treating it as "skip this key" is right for the first and
wrong for the second, which is why the reason is published rather than inferred.

`constraint: null` still means no check is possible: a domain newtype, or a type this crate does
not recognise. Inventing one would reject values the image accepts, which is the one thing a schema
here must never do.

`ExternalVar::constraint` sets both by hand for a type the crate cannot interpret — a duration, a
connection string — and the derive leaves whatever it finds alone.

**The file layers are a blunter question.** A key-named file in the secrets directory and a `_FILE`
target both deliver their contents as strings with no parse, and `Figment::extract` does not coerce
a string into a number or a boolean. A key whose **`constraint` is not a string type** therefore
**cannot be supplied by either, whatever the file contains** — not "must match a pattern", cannot
be supplied at all.

Keyed on `constraint`, not on `text_form`: that field answers what parse the *environment* layer
needs, and the two differ for every type whose `Deserialize` parses a string. An `IpAddr` key is
`text_form: unknown` — no pattern here describes an address — and it is a string in the document
and mounts from a secrets file perfectly well.

They also *read* differently, which is a third rule and not the table above. Trailing `\r` and
`\n` are stripped and no other whitespace is — every editor and every YAML block scalar adds a
line ending nobody meant as part of the value, whereas a trailing space can be a real character of
a real password. So `"x\n"` supplies a `char` key and `"x "` does not:

| layer | read |
|---|---|
| environment | trim all surrounding whitespace, then the form's read |
| secrets file, `_FILE` target | strip trailing line terminators, and nothing else |
| the document | nothing; it is already a parsed value |

Which is enough for a consumer holding a rendered `Secret` to check its **values** as well as its
file names: strip the trailing line terminators and apply `constraint`. Not `text_constraint` —
that is the one that looks right because it takes a string, and it is exactly wrong here, because
it permits the surrounding whitespace an environment spelling may carry and a file keeps. That is deliberate: those layers exist to carry secrets, and a
secret is an opaque byte string. A chart mounting `isr__ttl_secs` as a secret file has made a
mistake no file contents can fix, and a validator can say so from `constraint` alone.

## The half no derive can see

A service reads variables that are not its configuration. `PORT`, `IP` and `RUST_LOG` belong to the
Dioxus toolchain, which reads them before any of these layers exist; a base image contributes
`PATH` and `SSL_CERT_FILE`. None carry the loader's prefix, so no `Describe` implementation can
report them — and a validator that flagged everything it could not account for would flag all of
them.

`External` is where those go, and it is deliberately a *positive* declaration rather than a
suppression list:

| | What it says | What a validator does with it |
|---|---|---|
| `External::var` | this image reads it, and here is its type | checks it exactly like a configuration key |
| `External::ignore` | nobody here owns it | skips it |
| `External::unknown` | what to do with everything else | `Reject` by default |

The difference matters. A declared `PORT` with `ty("u16")` means a chart passing `PORT: "http"`
fails the same gate that a chart passing `PORTFOLIO_ISR__TTL_SECS: "soon"` fails. An ignored `PORT`
is a variable the chart may misspell freely. Reach for `ignore` only where there is genuinely no
owner — an operator's `TZ`, the platform's `KUBERNETES_*` — and note that only a trailing `*` is a
wildcard, because every consumer of this document implements the matching itself and a pattern
language is a place for two implementations to disagree about what is exempt from a check.

`build` refuses eight things outright, all of them ways a contract could quietly stop being one:

- an external variable **carrying the loader's prefix** — everything in that namespace is a
  configuration key, and declaring one external would leave it governed and exempt at once;
- an ignore pattern **reaching into that namespace**, which is the same exemption through the other
  door and worse, because a pattern exempts everything it happens to cover rather than one named
  variable. That includes a pattern which does not carry the prefix but subsumes it: `ignore("PORT*")`
  against `PORTFOLIO_` reads as a pattern about the external `PORT` and disables the whole gate,
  one character from a spelling that is entirely correct. An *exact* `ignore("PORT")` is fine — it
  matches that name and nothing else, and no key is spelled that;
- an external variable **colliding with a spelling the loader reads**, which is the first case
  reached through a `reserve`d name;
- an ignore pattern **covering a spelling the loader reads**, which is the second case reached the
  same way. The prefix is not the whole namespace: a key's environment spelling is derived from the
  prefix, but `config_var`, `secrets_dir_var` and `reserve` all take arbitrary names, so
  `ignore("CREDENTIALS_*")` against `secrets_dir_var("CREDENTIALS_DIR")` would exempt the variable
  that decides where every credential is read from;
- an external variable **declared twice**, on `Schema::merge`'s reasoning: refusing to build beats
  picking one of two descriptions;
- a **secret carrying a default**, in either order the two were declared in, and anywhere in the
  document. Nothing here produces that pair, but this is the point the document crosses into a
  public registry, and "no code path produces it" is a weaker guarantee than "the type will not
  carry it";
- an **empty prefix**, which would make step 4 of the list below fire for every variable on the
  container — every name begins with the empty string — so steps 5 and 6 would never be reached
  and a declared external surface would never be read. The deeper reason is that a prefixless
  loader cannot tell its own namespace from the machine's, and that distinction is what every gate
  rests on;
- a key whose environment spelling is **another key's `_FILE` variable**. With `token` and
  `token_file` both present, setting `<PREFIX>TOKEN_FILE` fills `token` from the file it names
  *and* fills `token_file` with the path — one variable, two keys — and a validator classifying
  that variable stops at the first. Publishing a contract that cannot describe an effect is worse
  than publishing none, because every gate downstream would pass. The application renames the
  field.

## How a validator reads it

Normative, and an ordered list because it has to be — two consumers running these in a different
order disagree about whether a deployment is valid, which is the failure the single wildcard form
exists to prevent, reached through evaluation order instead of pattern syntax. For each environment
variable on a container, first match winning:

1. one of `schema.loader[].env` — a variable the loader reads to decide what the layers are. Valid.
2. some `schema.keys[].env` **or one of that key's `env_aliases`** — that key, from the
   environment layer. Check it in two steps; see below.
3. some `schema.keys[].env_file` **or one of its `env_file_aliases`** — that key, by
   indirection. The value is a path, so neither constraint applies; what applies is that the path
   is mounted.
4. anything else beginning with `schema.dialect.prefix` — **reject.** A key spelling nothing in the
   image reads. Neither `external.env` nor `external.ignore` can reach this step, because `build`
   refuses both when they carry the prefix.
5. some `external.env[].name` — check it the same two ways, against that entry's
   `text_constraint` and `constraint`.
6. some `external.ignore` pattern — skip it.
7. otherwise — `external.unknown`.

That list is repeated verbatim in `External`'s own documentation, and the two must be edited
together: two normative statements that disagree is the same defect as none, and harder to notice.

The alias spellings in steps 2 and 3 matter more than they look. A key with `#[serde(alias = "…")]`
answers to every one of them — measured, in the environment layer and in the secrets directory
alike — so a chart still using a name kept alive by an alias is a *correct* deployment. Publishing
only the canonical spelling would send it to step 4 and reject it, turning the shim that makes a
rename safe into the thing that fails the gate. `env_aliases`, `env_file_aliases` and
`secrets_file_aliases` are those spellings, derived by the same rules as the canonical three
because a derivation left to prose is one each consumer gets differently wrong.

**Steps 2 and 5 are two checks, and both are needed.** A variable holds text and a configuration
holds a value:

1. **Form.** The text must satisfy `text_constraint`, when there is one. `"http"` is not an
   integer in any spelling, and this is the check that says so.
2. **Range.** *Read* the text according to `text_form`, then check the result against
   `constraint`. This is where `minimum`, `maximum`, `minLength` and a document-space `enum` live,
   and it is the only step that can reach them: a pattern matches characters, so `99999` is a
   well-formed integer and only a bound catches it not fitting a `u16`.

| `text_form` | read |
|---|---|
| `integer` | trim, drop a leading `+`, parse as an integer |
| `boolean` | trim, compare to `true` |
| `choice` | trim |
| `structured` | trim, parse as a TOML literal |
| `text`, `unknown` | trim |

**Every read begins by trimming**, because the environment layer trimmed before it parsed anything
— measured, and it holds for a plain `String` and a `char` as much as for an integer. A read that
skipped it would refuse `" x "` for a `char` key against a `minLength` of 1, on a value that loads.

`text_form` is what says which read, never the shape of `constraint`. `text` and `unknown` still
reach `constraint`, because their read is a trim rather than nothing — which is how a `char` key's
`minLength`/`maxLength` is checked, and where a future pattern for a `Uuid` or an address would
apply.

Skipping the second leaves every bound in the document decorative: a deployment that passes every
gate and fails at boot. Applying `constraint` to the raw text instead rejects `"0"` for an integer
key — a correct deployment refused.

**A 64-bit range is not checkable from this document at all.** `u64::MAX` is not representable as
an IEEE double, so no `maximum` is published rather than one that is a different number than the
type accepts. A `u64` key given `18446744073709551616` satisfies everything here and still fails to
load; loading the configuration with the real binary is what closes that, and no arrangement of
these fields would.

## What the contract deliberately cannot say

Step 4 also catches what a *cluster* injects into the prefix. Kubernetes service links inject
`<SERVICE_NAME>_SERVICE_HOST`, `<SERVICE_NAME>_PORT` and five more per Service in the namespace,
and the service name is the release name — which an image cannot know. A release called
`portfolio` produces `PORTFOLIO_SERVICE_HOST` and `PORTFOLIO_PORT` against a `PORTFOLIO_` prefix; a
release called `staging-portfolio` produces names outside it entirely. No declaration written at
build time is right for both, which is why there is no API for it.

**Set `enableServiceLinks: false` on the pod.** It is the deployment's business, and the deployment
does know the release name. It is also not merely a validation nuisance: `PORTFOLIO_PORT` is a
spelling of the key `port`, so with service links on, a Service named after the release *supplies*
that key from the environment layer, outranking the mounted file. That is a live misconfiguration
this document cannot fix and will not hide.

`Unknown::Reject` is the default and it is not free for the same reason. A pod carries `HOSTNAME`
from the runtime and `KUBERNETES_SERVICE_HOST` and its relatives from the API server, even on a
`scratch` image running one static binary. Those are what `ignore` is for; reaching for
`Unknown::Warn` instead gives up the whole gate to tolerate six names.

## Getting it onto the image

`Contract::to_json` is byte-stable: the same source tree produces the same bytes, so the document
can be hashed, and the hash is what ties three copies of it together.

```dockerfile
# Generate it in a builder stage, on the toolchain that is already there.
FROM builder AS contract-builder
RUN cargo run --features config-schema --example config-schema -- --format contract > /out/contract.json

FROM scratch AS runtime
# Embed it, so the image is self-describing with no registry at all.
COPY --from=contract-builder /out/contract.json /config/contract.json

# Discovery, from the config blob alone. `--format dockerfile` emits exactly this region,
# markers included — paste it, never retype it.
# terrace-config:labels:begin
LABEL dev.terrace.config.contract.version="1" \
      dev.terrace.config.contract.path="/config/contract.json" \
      dev.terrace.config.prefix="PORTFOLIO_"
# terrace-config:labels:end
```

The markers are what a drift check cuts on. Cutting by line count instead — `grep -A2
'^LABEL dev\\.terrace'` and its relatives — reads correctly right up until a fourth label is
added, and then compares two of three lines and passes.

**All three labels are constants for a service**, which is what lets them be a plain `LABEL` block:
no build argument to interpolate, and no host-side run of the generator to feed `--label`. That
last one is the trap in a multi-stage build — the document is produced *inside* a builder stage,
where the `docker build` command line cannot reach it, so feeding `--label` means running the
generator twice.

There is deliberately no label carrying the document's hash. It would buy a cross-check — that the
embedded file and the attached artifact are the same document, catching a pipeline that attached a
stale one — but that is a failure of the *build*, and the build is the one place holding both
copies locally and able to compare them for nothing. Every consumer downstream reads the registry
artifact, whose bytes the registry content-addresses. Publishing the assertion would make all of
them carry a field none of them need, and it was the only label that had to be dynamic.

A `LABEL` key cannot be interpolated from anything, so hand-writing the block is unavoidable —
which makes checking it the thing that matters. Check the **image**, not the Dockerfile:

```bash
crane config "$image" | jq -c '.config.Labels' > labels.json
# then, in a test or a small binary:
#   let labels = schema::cli::verify::labels_from_json(&read_to_string("labels.json")?)?;
#   contract.verify_labels(DEFAULT_PATH, &labels)?
```

`Contract::verify_labels` takes what `docker inspect` or `crane config` reports and names every
label that is missing or wrong, ignoring the `org.opencontainers.image.*` and base-image labels
around it. `verify::labels_from_json` is the reader in front of it, and it refuses the two inputs
that otherwise look like success: a `null`, which is what reading the wrong JSON path yields, and
anything that is not an object at all. Checking the built image rather than the source catches what a diff cannot: a build
argument that failed to interpolate, a label a base image overrode, a `LABEL` line deleted on the
branch that was not the one reviewed. Run it after the build and before the push, where a failure
costs a retry instead of a release.

Then, after the push, attach it to the digest:

```bash
oras attach --artifact-type application/vnd.terrace.config-schema.v1+json \
  "ghcr.io/you/portfolio@${DIGEST}" contract.json:application/json
```

That copy is the one a pipeline fetches — attached to the digest the chart pins rather than to a
tag that can move, two small HTTP requests rather than a layer pull, and signable. `ARTIFACT_TYPE`
is a constant here so the producer and the consumer cannot spell it differently.

Nothing inside the document names the image, and that is deliberate. The tie is the attachment: ask
a digest for its referrers of this artifact type and whatever comes back is that digest's contract,
by construction, and the registry content-addresses those bytes. A field claiming a digest could
only be written after the push, changing bytes that were already committed.

What a consumer does check is that the image and the document belong together — the image's
`dev.terrace.config.prefix` against the document's own dialect. That is `Contract::verify_labels`
run from the far side, and a mismatch is a build to refuse rather than a copy to prefer.

## Keeping the checked-in copy honest

The same `git diff --exit-code` gate the reference table gets, for the same reason and with more at
stake — a chart is about to be validated against this:

```yaml
- run: cargo run --features config-schema --example config-schema -- --format contract > docs/config.contract.json
- run: git diff --exit-code -- docs/config.contract.json
```

A renamed key then shows up in the diff of the pull request that renamed it, which is the cheapest
possible warning that a chart is about to break.

Both gates and the image check together are one action, if you use GitHub Actions:

```yaml
- name: Config contract
  uses: TimSchoenle/actions/actions/rust/config-contract@<sha> # tag=actions-rust-config-contract-v1.0.0
  with:
    features: config-schema
    image: myservice:test    # omit to run only the two checks that need no image
```

It renders the contract, its labels and its `LABEL` block from one run of one generator over one
source tree — so the three cannot disagree with each other — then diffs the Dockerfile marked
region, diffs the committed document, and checks the built image labels, reporting every fault
rather than the first. Each check is skipped by emptying its input.
