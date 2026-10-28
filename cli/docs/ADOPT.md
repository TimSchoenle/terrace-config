# Adopting keys a new contract adds: `adopt`

How `adopt` writes the chart values, write-offs and template lines for keys an image release adds,
and which cases it leaves to a person.

When an image release adds a setting, `bindings` fails because no chart value reaches it. `adopt`
writes the repair for the cases that need no judgement:

- an ordinary key gets a chart value, with its type, default and description taken from the
  contract, and the marker that binds it;
- a credential gets a value delivered as a file, with no marker, plus its `unbound` write-off and
  its `credentials` row;
- a reserved key gets its write-off and no value.

The template lines that project a new value are appended to `derivedConfig` and `secretData`.
That only happens where the define does not already write the key's root, and, in a
multi-document chart, only where the `$service` guard can be read from the declaration. Anywhere
else the lines are printed for a person to place.

```bash
terrace-contract adopt                                # plan every enrolled chart and print it
terrace-contract adopt portfolio --key sentry.debug   # one chart, only the named keys
terrace-contract adopt --check                        # exit 1 when any enrolled chart owes a key
terrace-contract adopt --diff                         # the unified diff --write would apply
terrace-contract adopt --write                        # apply it
```

`adopt` never touches a value that already exists. A key whose chart value is already there is
refused, and the refusal names the marker to write by hand. Only a person can say which key an
existing value feeds. A chart whose markers are broken is blocked and nothing is planned for it.

Every edit inserts whole lines and changes no existing byte: each block goes after the last bound
sibling of the value it belongs with, CRLF files stay CRLF, and YAML is never re-serialised.
`--write` stages every file of every chart before it replaces any of them, and puts back what it
replaced if a later replacement fails, so a run that fails part-way leaves the tree as it was. It
refuses to write a file that resolves outside its chart. `--diff` writes nothing. Its output applies
with `patch -p0` from the directory `adopt` ran in, and it lists blocked charts and refused keys on
stderr.

Exit codes:

- Without `--check`: `1` when a key was refused, a chart is blocked, or the run failed; `0`
  otherwise.
- With `--check`: `1` when anything is owed; `2` when the run could not answer.
- Both: `2` for a chart name that is not a chart, or `--key` without a chart.

`adopt` is not part of `sync`. Writing a setting into a chart is a change somebody should read.
