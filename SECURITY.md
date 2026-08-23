# Security policy

## Reporting a vulnerability

Do not open a public issue. Use GitHub's private vulnerability reporting on this repository,
under the Security tab, which opens an advisory only the maintainers can read:

<https://github.com/TimSchoenle/terrace-config/security/advisories/new>

Include what you did, what happened, and which feature set was compiled. A report against
`--all-features` and one against `--no-default-features --features loader` are different reports,
because the layers, the supervisor and the schema derive share almost no code.

## Supported versions

The crate is pre-1.0 and distributed as a git dependency, so there is no maintenance branch to
backport to. A fix lands on `main` and goes out in the next tag, and the remedy is to move the
`tag = "…"` in your manifest. Only the newest tag is supported.

## What a report is about

The crate handles credentials, so the interesting failures are the ones that move a secret
somewhere it was not meant to go:

- A secret value reaching a log, a `Debug` output, an error message or a panic message. No type
  here prints one, and `Terrace::explain` holds none at all, so any counter-example is a defect.
- A layer that silently supplies nothing where it should have supplied a key. This is what the
  secrets-directory provider gets wrong most easily, and a service that boots on compiled
  defaults instead of a mounted credential is a live incident rather than a crash.
- A path outside the configured secrets directory or config directory being read.

Panics on hostile input are fuzzed rather than reported: `fuzz/` carries an oracle per layer, and
a reproducing input is more useful as a seed than as an advisory. Open a normal issue with the
input if a committed seed does not already cover it.
