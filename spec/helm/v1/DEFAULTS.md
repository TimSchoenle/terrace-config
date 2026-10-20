# Declaration defaults

Every value `terrace-contract` supplies for a key that a chart's `config-contract.yaml` leaves out.
A key that is written always wins over its default, and writing the default value explicitly means
the same as leaving the key out.

These defaults are part of layer version 1. Changing one is a breaking change, made only in a new
layer version, because a chart that relies on a default has written nothing that would show the
change. `terrace-contract config show --chart <chart>` prints a chart's declaration with every
default written out and names the fields the defaults supplied.

## Document fields

| Field | Default when left out | Notes |
|---|---|---|
| `source.kind` | `ConfigMap` | |
| `source.selector` | `app.kubernetes.io/instance: <chart>` | `<chart>` is the chart's directory name. See [Release name](#release-name). |
| `source.key` | `config.toml` | |
| `source.format` | from the extension of `source.key`: `.json` is `json`, `.yaml` or `.yml` is `yaml`, anything else is `toml` | The extension is compared case-insensitively. |
| `images` | `[{values: image, contract: contracts/<name>.json}]` | `<name>` is the document's `name`. An explicit empty list is refused. |
| `images[].contract` | `contracts/<name>.json` | Only when `images` has exactly one entry. With two or more, every entry must name its contract. |
| `consumers` | inferred, see [Inferred consumers](#inferred-consumers) | Only for a `ConfigMap` or `Secret` source. Any other kind must list its consumers. An explicit empty list checks no workload. |
| `tests` | `probe: config`, no baseline, no prerequisites, nothing unrouted | Only on a chart with `tests: true`. |

## Chart fields

| Field | Default when left out |
|---|---|
| `bindings` | `false`: the chart is not enrolled in the marker check |
| `tests` | `false`: the chart is not enrolled in the round-trip suites, unless a `contract-tests.yaml` enrols it (see [DECLARATION.md](DECLARATION.md#round-trip-enrolment)) |
| `unconfigured`, `unbound`, `credentials` | empty |
| a document's `exempt`, `restart` | empty |

## Release name

The default selector matches `app.kubernetes.io/instance`, the label charts in this family set to
`.Release.Name`. Neither `nameOverride` nor `fullnameOverride` changes it, so it names the same
object under every values file.

The value it is matched against is the chart's directory name, because the rendered fixtures the
gates read are produced with `helm template <chart>`, which uses the chart's name as the release
name. A repository that renders with a different release name must write `source.selector`.

## Inferred consumers

When `consumers` is left out, the consumers are found again in each rendered values file. A
workload consumes the document when its pod template mounts the source object, and the containers
that read it are those, init containers included, whose `volumeMounts` reference the mounting
volume. The workload kinds searched are `CronJob`, `DaemonSet`, `Deployment`, `Job`, `Pod`,
`ReplicaSet` and `StatefulSet`.

A volume mounts the source object when it names it:

- a `configMap` volume whose `name` is the object's name, for a `ConfigMap` source;
- a `secret` volume whose `secretName` is the object's name, for a `Secret` source;
- a `projected` volume with a `configMap` or `secret` source of that name.

Only mounts count. A workload that reads the object through `env` or `envFrom` is not found, and
has to be listed.

Inference runs per values file because a values file can switch a reader on or off. When the source
object renders and no workload mounts it, `check` warns that nothing consumes that configuration.
