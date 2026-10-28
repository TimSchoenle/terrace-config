# Charts whose images reload

How a chart decides which configuration changes restart its pods, from what each image's contract
says it applies without a restart.

An image may publish that it applies a change without restarting (`schema.reload`), and which keys
only a restart applies (`keys[].reload`). A chart that hashes its whole configuration into a pod
annotation throws that reload away; one that hashes none of it leaves every restart-only key changed
on disk and never applied. `reload` writes the part each chart should hash:

```bash
terrace-contract reload --charts charts          # writes charts/<chart>/templates/_config-reload.tpl
terrace-contract reload --check --charts charts  # CI: fails when a file is behind its contracts
```

`check` then holds each render to it:

- a long-running workload reading a document with restart-only keys must carry a `checksum/` pod
  annotation;
- no key the image applies live may arrive through a `subPath` mount or an immutable object, which
  the kubelet never updates.

A chart may add keys to the restart set with a `restart:` entry per document in
`config-contract.yaml`, each with a reason, and never remove one. `explain` prints what a change to
each setting costs.

Nothing changes for a chart until one of its images declares `mode: rebuild`. The rules are in
[`FORMAT.md`](../../spec/v1/FORMAT.md#reloading).
