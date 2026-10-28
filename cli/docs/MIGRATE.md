# After a contract removes something: `migrate`

How `migrate` finds the template, test and CI lines that still use a key or element property a
refreshed contract removed.

A refresh writes values for the keys an image added. It cannot follow a removal, because the lines
still using what went away are in templates, hand-written test suites and CI values files, which no
writer edits. `migrate` lists those lines:

```bash
terrace-contract migrate tankovault --since "$(git merge-base HEAD origin/main)"
terrace-contract migrate --json                     # every chart, against origin/main
```

```text
==> hand migration required: the contract took these away, and no generator edits
    the templates, hand-written tests or CI values that still use them
  tankovault: legal.dir: key removed (chart value legal.dir)
      charts/tankovault/templates/_config.tpl:515: dir: {{ $ctx.Values.legal.dir | quote }}
  tankovault: legal.documents: element property 'sources' removed (chart value legal.documents)
      charts/tankovault/tests/legal_test.yaml:58: legal.documents.privacy.sources.en: /mnt/legal/privacy.en.md
```

It reports two kinds of change: a contract key that was removed, and a property removed from the
schema of a key's elements. It compares the vendored contracts with the revision given in
`--since`, in the same way as `diff`. It reads the chart's markers from `values.yaml` **as it was
at that revision**, because by the time anyone looks, the value a removed key was bound to has
usually been deleted. It searches `templates/`, `tests/*.yaml` and `ci/*.yaml`, and skips the
generated `contract_roundtrip_*_test.yaml` suites, because running `tests` regenerates those.

Pass the commit the branch started from, not `HEAD`. On a bump branch the refreshed contract is
usually committed already, so nothing has changed since `HEAD`.

The search is plain text, so a hit is a place to look rather than proof that the line is wrong.
Every removal is listed, including ones nothing references, so you can confirm that a migration is
complete. The exit code is `0` when no line still uses a removal, `2` when one does, and `1` when
the command could not answer (an unknown revision, a chart name that matches no chart, or a
contract or marker it cannot read). These are the same three outcomes as `diff --exit-code`.
