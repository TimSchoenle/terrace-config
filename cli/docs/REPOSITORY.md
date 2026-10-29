# Chart repository settings and gates

How a chart repository configures `terrace-contract` in `terrace.yaml`, narrows its gates to the
charts a change touched, runs every step with `sync`, and checks that templates render what their
markers bind.

## Settings: `terrace.yaml`

A chart repository keeps its settings in `terrace.yaml` at the root of its Git repository, instead
of repeating flags in every script:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/TimSchoenle/terrace-config/main/spec/helm/v1/repo.schema.json
version: 1
cli: "0.8"
helm:
  charts: charts
  firstParty: [ghcr.io/example/*]
  signer: https://github.com/example/[^/]+/.github/workflows/release.yaml@refs/heads/main
```

```bash
terrace-contract config init                    # writes one, copying .github/configs/first-party-images.txt into it
terrace-contract config show                    # every setting, its value, and where it came from
terrace-contract config show --chart portfolio  # the chart's declaration with every default written out
terrace-contract doctor                         # the settings, the pinned CLI and the tools, one line each
```

A flag wins over its environment variable, which wins over the file, which wins over the default.
The file is optional; without one, every setting takes its flag, variable or default. A key this
release does not read is a warning, and fails the command under `--strict` or `TERRACE_STRICT=1`.
Every key, its flag and its default are in [`spec/helm/v1/REPO.md`](../../spec/helm/v1/REPO.md).

## Checking only the charts a change touched

By default, `check`, `bindings`, `shapes`, `readme` and `tests` read every chart under `--charts`.
Each of them also takes chart names after its options, and then reads only those charts:

```bash
terrace-contract bindings --format github portfolio netcup-offer-bot
terrace-contract tests --check portfolio
```

A name that matches no chart directory exits `2`. A misspelt name would otherwise select nothing,
and a gate that checks nothing passes.

`scope` works out the names from Git. It prints the application charts that the commits since a
revision touched, measured from the merge base with `HEAD`, on one line:

```bash
terrace-contract scope --since origin/main          # e.g. "portfolio tankovault"
terrace-contract scope --since origin/main --json   # the same, with the reason it widened
```

The line is empty, meaning every chart, whenever narrowing could miss a chart the change breaks:

- a changed path outside the charts' own directories, such as the pinned CLI, `terrace.yaml` or a
  workflow;
- a change to a chart that is a library at the base or at `HEAD`, so deleting a library, or turning
  one into an application chart, still widens;
- a change that cannot be read, such as in a shallow clone.

Why it widened goes to stderr. It can therefore widen a gate and never narrow one by mistake.

## One command for CI: `sync`

`sync` runs the steps a chart repository needs in the right order, so a pipeline or a person runs
one command instead of a list of them.

```bash
terrace-contract sync                                # pull, then rewrite shapes and README references
terrace-contract sync --no-pull                      # the same from the contracts already vendored
terrace-contract sync --check --since origin/main    # every offline gate, on the charts the change touched
terrace-contract sync --check --manifests rendered   # and `check` on a render, too
```

Without `--check` it writes: `pull`, then `shapes`, then `readme`, each reading what the one before
it wrote. It does not write the round-trip suites, because a generated test that lands without its
author reading it is a test nobody has read; run `tests` for those.

With `--check` it writes nothing and needs no network. It runs `bindings`, `shapes --check`,
`readme --check`, `tests --check` and, when `helm.firstParty` is set or
`.github/configs/first-party-images.txt` exists, `coverage`. With `--manifests`
it also runs `check` on that render. Every step runs even when an earlier one failed, so one run
reports everything. The exit code is `0` when every step passed, `1` when a step found something,
and `2` when a step could not run. `--format github` puts each step in a collapsible log group and
writes the findings into the job summary.

## Markers a template never renders

`bindings` holds every `# @config` marker in an enrolled chart (`bindings: true` in
`config-contract.yaml`) against the contract it names. It also checks that the chart's templates
render each bound key at all: for every `projection`, `structured` and `composed` marker, the last
segment of the contract key must appear in some `templates/*.tpl` file, either as a mapping key
(`interval_secs:`) or as a quoted `dict`/`set` argument (`"interval_secs"`). Otherwise a value can
be bound, typed and documented while the template renders nothing for it.

The check is a plain text search. A hit means the key's name is rendered somewhere, and does not
mean it is wired to the right value. Because it is part of `bindings`, `sync --check` runs it with
no extra step.
