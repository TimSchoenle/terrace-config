# `terrace.yaml`

One file at the root of the Git repository, holding the settings `terrace-contract` would otherwise
take from flags, environment variables or built-in paths. Every key except `version` is optional,
and a repository with no file behaves exactly as it did before the file existed.

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/TimSchoenle/terrace-config/main/spec/helm/v1/repo.schema.json
version: 1
cli: "0.8"

helm:
  charts: charts
  firstParty:
    - ghcr.io/example/*
  signer: https://github.com/example/[^/]+/.github/workflows/release.yaml@refs/heads/main
```

## Finding the file

The CLI looks for `terrace.yaml` in the root of the Git repository that contains the working
directory: the nearest ancestor holding a `.git` entry. It does not look anywhere else, so a file in
a subdirectory is never picked up by accident. `--config <file>` or `TERRACE_CONFIG` names a file
explicitly and skips the search. A named file that does not exist is an error; a repository with no
file is not.

Relative paths in the file are resolved against the directory that holds it, not against the
working directory, so a command gives the same answer from anywhere in the repository. Once a file
exists, the built-in default paths below are resolved the same way. Without one they stay relative
to the working directory, as they were before this file existed.

## Resolving a value

Each setting is taken from the first of these that provides it:

1. the command-line flag;
2. the environment variable;
3. `terrace.yaml`;
4. the built-in default.

`terrace-contract config show` prints every setting with its value and the layer it came from, and
`terrace-contract config init` writes a new file, moving the patterns of an existing first-party list
file into `helm.firstParty`.

## Keys

| Key | Type | Flag | Environment | Default |
|---|---|---|---|---|
| `version` | `1` | | | required |
| `cli` | `MAJOR.MINOR` or `MAJOR.MINOR.PATCH` | | | none |
| `helm.charts` | path | `--charts` | `TERRACE_CHARTS` | `charts` |
| `helm.firstParty` | list of repository patterns | `--first-party <file>` | | the patterns in `.github/configs/first-party-images.txt` |
| `helm.signer` | regular expression | `--signer` | `CONTRACT_SIGNER` | none; `pull` refuses to run without one |

`version` is the layer version, and the only key a CLI interprets before the rest. A version the
running CLI does not support is refused with exit code 2.

`cli` is the release this repository is checked with. The setup action installs it, and
`terrace-contract doctor` reports a running CLI whose major and minor differ from it. A
`MAJOR.MINOR` pin takes every patch release.

`helm.firstParty` uses the pattern language of a contract's `external.ignore`: a trailing `*` matches
any suffix, and anything else is an exact repository. It is matched against `image.repository` as
written in a chart's values. The default file is read only when neither the flag nor this key is
set, and is kept for repositories that predate this file.

`helm.signer` has no default because the identity that signs an organisation's images is that
organisation's policy. A default would let a fetch accept a signature nobody chose to trust.

## Keys this version does not define

A file may carry keys a newer version of this layer defines. The running CLI does not read them,
and says so: each unknown key is reported as a warning naming the key, and naming the `cli` pin
when the file declares one newer than the running CLI. Under `--strict` or `TERRACE_STRICT=1` an
unknown key fails the command with exit code 2, so a repository cannot rely on a setting its CLI
ignores. `terrace-contract doctor` always reports an unknown key as a failure.

Keys starting with `x-`, at the top level or inside a section, are reserved for extensions. The CLI
never reads them and never reports them.
