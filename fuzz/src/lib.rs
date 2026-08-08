//! The fuzz oracles, as an ordinary library.
//!
//! Each `fuzz_targets/*.rs` binary is a shim over one function in [`oracle`]. The bodies live
//! here rather than in the targets so they can be **replayed without libFuzzer** — see
//! `tests/seeds.rs`, which runs every committed seed through the matching oracle on a plain
//! `cargo test`.
//!
//! That split is not only a convenience. `cargo fuzz` needs `-Z sanitizer=address`, which is
//! nightly-only and, on Windows, needs an `AddressSanitizer` runtime that ships with Visual
//! Studio rather than with rustup. An oracle that can only run under that toolchain is an
//! oracle nobody checks. Here, the seeds are a regression suite that runs anywhere, and the
//! fuzzer is what discovers new inputs to add to it.

// `figment::Jail::try_with` fixes the closure's error type to the large `figment::Error`, and
// every oracle is one such closure — so the expectation belongs here rather than three times
// over.
#![expect(
    clippy::result_large_err,
    reason = "figment::Jail::try_with fixes the closure's error type"
)]

pub mod oracle;
pub mod support;
