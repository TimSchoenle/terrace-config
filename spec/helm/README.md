# The Helm layer

`spec/v1/` is the contract an image publishes. This directory is the other half: the files a
**chart repository** writes so that `terrace-contract` can hold its charts to the images they deploy.
None of it is part of the wire format, and nothing here may leak into `spec/v1/`.

It lives in this repository rather than in any chart repository because the CLI embeds these
schemas and enforces these rules. A convention and the code that enforces it change in the same
pull request, together with the cases that test them. A chart repository is a consumer of this
directory like any other.

## Versioning

Directory-versioned, independently of `terrace_contract`. Each governed file declares the layer
version it is written against, and the CLI reads that field before anything else.

| File | Version field | Specified in |
|---|---|---|
| `terrace.yaml` at the repository root | `version` | [`v1/REPO.md`](v1/REPO.md), [`v1/repo.schema.json`](v1/repo.schema.json) |
| `charts/<chart>/config-contract.yaml` | the `version` of `terrace.yaml` | [`v1/DECLARATION.md`](v1/DECLARATION.md), [`v1/DEFAULTS.md`](v1/DEFAULTS.md), [`v1/declaration.schema.json`](v1/declaration.schema.json) |

Within a version, a key is added only when an older CLI can ignore it without changing any gate's
verdict: it enables something new or narrows a report, and never relaxes or reinterprets an existing
check. Anything else, including a changed default, is a new directory (`v2/`). A CLI that introduces
a new layer version keeps reading the previous one for at least one minor release, and warns when it
does.

## Layout

```text
spec/helm/
  README.md            this file
  v1/
    REPO.md                   terrace.yaml: every key, its default, and how a value is resolved
    repo.schema.json          validates terrace.yaml; embedded in terrace-contract
    DECLARATION.md            config-contract.yaml: every key, and round-trip enrolment
    DEFAULTS.md               every default a declaration may rely on; changing one is breaking
    declaration.schema.json   validates config-contract.yaml, for editors
```

This directory does not specify the `@config` marker language.

## Using the schemas

The CLI embeds `repo.schema.json` and validates `terrace.yaml` against it offline. It reads
`config-contract.yaml` with its own stricter reader, whose messages name the key at fault, and its
tests hold `declaration.schema.json` to the same keys and refusals.

An editor validates either file through the `yaml-language-server` modeline that
`terrace-contract config init` and `terrace-contract helm init` write:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/TimSchoenle/terrace-config/main/spec/helm/v1/repo.schema.json
# yaml-language-server: $schema=https://raw.githubusercontent.com/TimSchoenle/terrace-config/main/spec/helm/v1/declaration.schema.json
```

The `$id` inside each schema is an identifier, not a promise that something answers at that URL.
