# `tier-1-spellings`

A producer that states its spellings and does not derive them, because its loader does not.

`RELAXED_SERVERPORT` does not follow from `server.port` and `__`. Neither does
`RELAXED_DATASOURCE_URL` from `data.source.url`. Both are what a binder with its own relaxed-binding
rules actually reads, and the producer publishes them because publishing is the only way a consumer
can know.

## What it pins

**Derive nothing the document states.** Everything reading `env`, `env_file`, `secrets_file` and
`unreachable` as published is tier 1 work and is unconditional — which is most of the toolchain, and
is why a tier 1 producer is usable by the gates on the day it emits its first document.

- The classification finds every key by the spelling the document gives, at every step.
- `cache.ttl` names no variable at all, and that is an ordinary answer rather than a gap to be
  filled in by deriving one.
- The one rule that reads `dialect.nesting_separator` is the dynamic-map legitimiser, and it can
  only turn a finding into silence. Against this document it finds nothing, so nothing changes.
- Mechanically: `grep -r nesting_separator cli/src` returns hits only where a rule says in its own
  documentation that it is making a tier 2 assumption.

## Source

Hand-written. A producer that reached tier 2 could not emit this, and that is the case: tier 2 is an
assumption, and a rule that makes it silently is wrong about every producer that does not.
