//! Shim over [`terrace_config_fuzz::oracle::kube`], where the oracle and its documentation live.
//!
//! The body is in the library so it can be replayed by `tests/seeds.rs` without libFuzzer —
//! `cargo fuzz` needs a nightly-only sanitizer, and on Windows an `AddressSanitizer` runtime that
//! ships with Visual Studio rather than with rustup. An oracle that only runs under one
//! toolchain is an oracle nobody checks.
//!
//! Gated on `cfg(fuzzing)`, which `cargo fuzz` sets and nothing else does. Without it this is a
//! binary with no `main`, and a plain `cargo test` in this directory fails to link it before it
//! ever reaches the replay suite.

#![cfg_attr(fuzzing, no_main)]

#[cfg(fuzzing)]
libfuzzer_sys::fuzz_target!(|data: &str| terrace_config_fuzz::oracle::kube::check(data));

#[cfg(not(fuzzing))]
fn main() {
    eprintln!(
        "built without --cfg fuzzing; run this through `cargo +nightly fuzz run kube` \
         or replay the corpus with `cargo test`"
    );
}
