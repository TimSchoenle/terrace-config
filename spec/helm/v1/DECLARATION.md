# `config-contract.yaml`

One file per chart, at `charts/<chart>/config-contract.yaml`, declaring the configuration documents
the chart renders, the images that read them, and which gates the chart is enrolled in. A chart
without the file is skipped by every gate. It is validated by
[`declaration.schema.json`](declaration.schema.json), and every default is listed in
[`DEFAULTS.md`](DEFAULTS.md).

A single-image chart that follows the defaults declares one document by name:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/TimSchoenle/terrace-config/main/spec/helm/v1/declaration.schema.json
bindings: true
tests: true
documents:
  - name: server
```

`terrace-contract helm init` writes this file for a new chart, and
`terrace-contract config show --chart <chart>` prints the declaration with every default written
out. The printed form is itself a valid declaration.

## Reading the file

The file is read strictly. It is written by hand in the chart repository, and its job is to be
exhaustive, so a key the CLI does not read is a typo and is refused, at every level. This is the
opposite of how a contract document is read, where an unknown field means the producer is newer.

The file follows the layer version of the repository's `terrace.yaml` (see [`REPO.md`](REPO.md)).
A key added to this file in a later release of the CLI is refused by an earlier one, so a
repository that uses it pins `cli` in `terrace.yaml` to a release that reads it.

## Top level

| Key | Type | Meaning |
|---|---|---|
| `documents` | list of [documents](#documents) | Required. An empty list opts the chart out, and then `reason` is required. |
| `reason` | string | Why the chart declares no documents. |
| `bindings` | boolean | Enrols the chart in the marker check: every contract key is bound to a chart value by a `@config` marker, or written off in `unbound`. |
| `tests` | boolean | Enrols the chart in the generated round-trip suites. See [Round-trip enrolment](#round-trip-enrolment). |
| `unconfigured` | list of values paths | Values paths at which the chart pins an image that reads no contract-described configuration. Values paths rather than repositories, because a chart can pin one repository twice. |
| `unbound` | list of `{keys, reason, documents?}` | Contract keys the chart binds no chart value to. `documents` scopes an entry to the named documents; left out, it covers every document that declares the key. A key may be written off once per scope. |
| `credentials` | list of `{key, value?, note?}` | Additions to the credential reference generated into the chart's README. `value` is the chart value an operator may set instead of creating the Secret; `note` is the condition under which the release needs the credential. Each entry carries at least one of the two, and each `key` appears once. |

The two enrolment switches are declared rather than inferred. Enrolment used to follow from the
chart carrying markers or a test file, and a chart that stopped carrying them dropped out of the
gate without a message.

## Documents

| Key | Type | Meaning |
|---|---|---|
| `name` | string | Required, unique in the chart. Every message names the document by it, and the default contract path is spelled from it. |
| `source` | `{kind?, selector?, key?, format?}` | The rendered object holding the document, and the key under its `data`. `selector` is a non-empty mapping of labels, matched instead of the rendered name because a name override moves the name. `format` is `toml`, `json` or `yaml`. |
| `images` | non-empty list of `{values, contract?}` | The images that read the document. `values` is the values path the chart pins the image at; `contract` is the vendored contract, relative to the chart directory. |
| `consumers` | list of `{workload: {kind, selector?}, containers}` | The workloads that mount the document and their containers that read it. An empty list is a written decision to check no workload. |
| `exempt` | list of `{values?, gates, reason}` | A rendered values file (or `*` for every one) that is not held to some gates. `gates` are `closed`, `document`, `env` and `files`. |
| `restart` | list of `{keys, reason}` | Keys the chart rolls its pods for although an image applies them live. The only direction in which a chart overrides an image. |
| `tests` | [suite block](#the-suite-block) | How this document's round-trip suite is generated. |

Every field except `name` has a default or is empty when left out. See [`DEFAULTS.md`](DEFAULTS.md).

Every `reason` is required wherever the key appears. An exemption, a write-off or a rollout without
a reason cannot be told apart from an oversight.

## Round-trip enrolment

`tests: true` enrols the chart: every document gets a generated suite. A document's `tests` block
says how its suite differs from the default one. A block on a chart without `tests: true` is
refused, because it would configure a suite that is never generated.

### The suite block

| Key | Type | Meaning |
|---|---|---|
| `probe` | dotted values path | The values path every case writes into. Default `config`, the operator-facing configuration tree. |
| `baseline` | mapping of dotted path to scalar | Values every case carries, under `probe`. Used when the chart refuses a combination that a single probe would produce. |
| `reason` | string | Why the baseline or probe path is needed. Required when either is set. |
| `prerequisites` | `{values, reason}` | Chart values every case needs before the chart renders, for example a reference to a credential. `values` maps a dotted chart values path to a value of any shape. No entry may sit inside the `probe` tree, and no two entries may overlap. |
| `unrouted` | list of `{keys, reason}` | Keys probed through the raw configuration tree instead of through their chart value, because the chart guards that value in a way a synthesised probe cannot satisfy. |

### `contract-tests.yaml`

Before this file carried enrolment, a chart enrolled by adding `charts/<chart>/contract-tests.yaml`,
a mapping of document name to the suite block above. The CLI still reads that file for a chart
whose declaration does not set `tests: true`. A chart that sets `tests: true` and also has the file
is refused, because the two could disagree. To migrate, move each entry into its document's `tests`
block, add `tests: true`, and delete the file. `config show --chart` prints the migrated form.

## Rules beyond the schema

The schema covers the shape of the file. The CLI additionally refuses:

- a document name declared twice;
- an `unbound` entry whose `documents` names a document the chart does not declare, or a key
  written off twice in one scope;
- an `images` entry without `contract` when the document has more than one image;
- a document without `consumers` whose source kind is neither `ConfigMap` nor `Secret`;
- a `prerequisites` entry inside the `probe` tree, two overlapping entries, or a `baseline` entry
  outside it;
- `tests: true` together with a `contract-tests.yaml`.

The CLI's messages name the file and the key, and are authoritative where they differ from an
editor's schema validation.
