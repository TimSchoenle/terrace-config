# `foreign-ty`

Every `ty` in this document is a type name from a language this repository is not written in.

`FORMAT.md` says what `ty` is: the producer's own name for the type, for a reader. It is **not
portable and nothing may switch on it.** A Java producer emits `java.time.Duration` where a Rust one
emits `u64`, and both are correct about their own program.

## What it pins

- No rule reads `ty` to decide anything. The property is checkable rather than asserted: this
  document and the same document with every `ty` removed produce the same findings against the same
  manifest — except for the one message that *prints* the type, which is a reader's aid and prints
  whatever is there.
- What the rules read instead is `text_form`, `constraint`, `text_constraint` and `values`, all of
  which the producer composed with the type in front of it, in a language this crate does not know.
- The moment one `match` arm spells a Rust type name, the toolchain is single-producer again. That
  is what this case exists to make fail.

## The near miss worth naming

The TOML rendering's placeholder was `ty`-keyed in the implementation it was ported from — a table
of Rust type names deciding which `<...>` to write. It reads the published `constraint` instead: the
same answer, arrived at by the producer that did have the type, and right for every producer.

## Source

Hand-written. A Rust producer cannot emit these type names and a Java one does not exist here yet,
which is exactly why the case has to be written rather than blessed.
