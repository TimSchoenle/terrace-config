//! Shim over [`terrace_contract_fuzz::oracle::render`], where the oracle and its documentation live.
//!
//! Gated on `cfg(fuzzing)`, which `cargo fuzz` sets and nothing else does. Without it this is a
//! binary with no `main`, and a plain `cargo test` in this directory would fail to link it before
//! reaching the replay suite.

#![cfg_attr(fuzzing, no_main)]

#[cfg(fuzzing)]
libfuzzer_sys::fuzz_target!(|data: &str| terrace_contract_fuzz::oracle::render::check(data));

#[cfg(not(fuzzing))]
fn main() {
    eprintln!(
        "built without --cfg fuzzing; run this through `cargo +nightly fuzz run render`          or replay the corpus with `cargo test`"
    );
}
