# `foreign-loader`

A document whose `producer.loader` is one this repository publishes no measured read table for.

**A consumer must skip the range step and say it skipped it.** `FORMAT.md` is explicit: the reads
it tabulates are normative for `producer.loader == "figment"` "and for nothing else. A consumer
meeting a loader it does not know MUST skip step 2 and say so."

## What it pins

- `reads_for("spring-boot")` is `None`, and the type is what makes that unforgettable — there is no
  default row to fall through to and nothing to forget to branch on.
- The **form** step still runs. `text_constraint` is what the producer published about its own
  reads, so holding text to it repeats the producer's statement rather than making one.
- The **range** step does not, so `server.port` is not held to its `maximum` of 65535. A gate that
  applied figment's reads here would refuse text Spring's `Binder` accepts, and that is the
  expensive direction: it stops a deployment that was correct, at a gate, with a false message,
  where the other direction fails at boot with a true one.
- The skip is reported once per document, at warning severity. Never a pass, and never a failure.

## Why `spring-boot` specifically

`CONFORMANCE.md` predicts it by name: "An implementation that hands naming to a binder with its own
relaxed-binding rules will not reach tier 2 without overriding it, and should not pretend to." It is
the first real producer for which a guessed read table would be wrong rather than merely unproven.

## Source

Hand-written. No producer in this repository can emit it, which is the point: the corpus has to
carry the documents a *consumer* must survive, not only the ones a producer here can write.
