# Starting a chart from its image's contract: `helm init`

How `helm init` writes the configuration half of a new chart from its image's contract, and the
placeholder protocol that lets a repository copy its own chassis around it.

A chart has two halves. The chassis is the same in every chart of a repository: `Chart.yaml`, the
workload templates and the README source. The repository keeps it as a template and copies it. The
configuration half comes from the image's contract, and `helm init` writes it:

| file | what it is |
|---|---|
| `contracts/<document>.json` | the image's contract, fetched and verified exactly as `pull` does it |
| `values.yaml` | the image pin, a block per setting with the marker that binds it, then `--chassis` |
| `templates/_helpers.tpl` | the partials that project the values into the document and the Secret |
| `config-contract.yaml` | the declaration: only what differs from the [defaults](../../spec/helm/v1/DEFAULTS.md), `bindings: true`, `tests: true` with a prerequisite per required credential, and a reasoned `unbound` entry per write-off |
| `ci/test-values.yaml` | the credentials the render guard demands, as values that can never authenticate |
| `tests/configmap_test.yaml` | a first suite for a person to extend |

```bash
terrace-contract helm init portfolio --image timschoenle/portfolio:v3.1.3@sha256:...           # print the plan
terrace-contract helm init portfolio --image timschoenle/portfolio:v3.1.3@sha256:... --diff    # the diff --write would apply
terrace-contract helm init portfolio --image timschoenle/portfolio:v3.1.3@sha256:... --write \
  --chassis .github/templates/chart/values.chassis.yaml
terrace-contract helm init redis --image library/redis:8.0 --no-contract --write              # no contract: the plain form
```

`--document` names the document in the declaration and defaults to the chart's name.
`--document-key` is the `ConfigMap` key it is rendered under and defaults to `config.toml`. The
signer comes from `--signer`, `CONTRACT_SIGNER` or `helm.signer`, as for `pull`. There is no way to
hand `init` a contract file: a contract that skipped verification would be trusted by every gate
downstream.

## The placeholder protocol

The copied templates have to name things only the contract knows: the environment prefix, the
nesting separator and the document key. They spell them `%%PREFIX%%`, `%%ENV_PREFIX%%` (the prefix
without its trailing separator), `%%SEPARATOR%%` and `%%DOCUMENT_KEY%%`, a form Helm never reads.
`init` fills them in every file of the chart. It skips `charts/` and `contracts/` at the top level,
and any file that is not UTF-8. Any other `%%NAME%%` still present is refused, because it is the
copy's to substitute and it would otherwise render as literal text. So the copy can be a `sed` over
the chart's name, description and versions, and nothing in it has to read JSON.

## The plain form

`--no-contract` writes `values.yaml` with only the image and the chassis, an empty `_helpers.tpl`
and an empty fixture. It writes no `ConfigMap`, no Secret and no `configMount`, because each
describes a loader nothing knows this image has. A first-party image, by the same list `coverage`
reads, still owes a declaration: `--reason` is required in that case, and is written as the
declaration's opt-out. A third-party image gets no declaration. The placeholders are filled from
the chart's name (`REDIS_`, `__`), since there is no dialect to read.

## It replaces nothing

Every file is planned before one is written. A file already there that says something else is
refused, and the error names each one. A file that already holds exactly what would be written is
left alone, so a second run owes nothing. The vendored contract counts as unchanged when only its
fetch time moved. A chart that already has a configuration is `adopt`'s to change.

After `--write`, it prints what is left for a person, in order:

- the grouping blocks whose description is a `TODO`, because a contract describes keys and not the
  blocks a chart groups them into;
- the values it had to invent, for keys the image requires and publishes no default for;
- the `unbound` reasons to read.

Exit codes:

- `--check`: `1` when anything would be written; `0` when the chart already holds exactly this;
  `2` when it could not answer.
- `--diff`: prints a `patch -p0` diff and exits `0`.
- Otherwise: `1` for any failure.
