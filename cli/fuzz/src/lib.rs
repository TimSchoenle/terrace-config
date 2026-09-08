//! The oracles, in a library so they can be replayed without libFuzzer.
//!
//! `cargo fuzz` needs a nightly sanitizer, and on Windows an `AddressSanitizer` runtime that ships
//! with Visual Studio rather than with rustup. An oracle that only runs under that toolchain is an
//! oracle nobody checks — so the bodies live here, `fuzz_targets/` is three shims, and
//! `tests/seeds.rs` replays the whole committed corpus on a plain `cargo test`, anywhere.
//!
//! The fuzzer's job is then to find new inputs worth committing, rather than to be the only thing
//! that ever runs them.

pub mod mutate;

/// One oracle per target.
pub mod oracle {
    pub mod conform;
    pub mod document;
    pub mod render;
}
