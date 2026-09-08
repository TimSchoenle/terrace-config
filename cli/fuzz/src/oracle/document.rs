//! The reader, and the two properties everything downstream depends on.
//!
//! This target takes the input as **raw JSON**, not as a mutation directive. That is deliberate
//! and it is the one place the other two are not enough: the reader's job includes refusing, and
//! the shapes it has to refuse cleanly are exactly the ones a mutation over a valid document never
//! produces — a truncated object, a string where a number belongs, an array at the root.
//!
//! Two properties, and both are load-bearing rather than decorative.
//!
//! **The envelope gate holds.** A document declaring a `terrace_contract` this build was not
//! written against must be refused whatever else it contains. `FORMAT.md` makes that a MUST for a
//! reason: misreading a document is worse than declining to read one, and every consumer decision
//! downstream is made on the assumption that this gate ran first.
//!
//! **Reading is a fixed point.** A document that parses must serialise to something that parses
//! again to the same thing. Without it `stamp` is unsafe — it rewrites a build's identity and
//! nothing else, and a reader that silently dropped or reordered a field would have it rewriting
//! bytes nobody asked it to.

use terrace_contract::{CONTRACT_VERSION, Contract};

/// Run the oracle over one input.
///
/// # Panics
/// On any finding, which is what a fuzzer is listening for.
pub fn check(data: &str) {
    let Ok(contract) = Contract::from_json(data) else {
        // A refusal is a valid outcome and most inputs reach it. The one thing worth asserting is
        // that a refusal is *quiet*: it returned rather than panicking, which the call above
        // already established.
        return;
    };

    // The gate, restated as an assertion rather than trusted. A reader that grew a path around its
    // own version check would still parse every valid document, so nothing else would catch it.
    assert_eq!(
        contract.terrace_contract, CONTRACT_VERSION,
        "a document declaring envelope {} was read by a build that reads {CONTRACT_VERSION}",
        contract.terrace_contract
    );

    let once = serde_json::to_string(&contract).expect("a read contract serialises");
    let twice = Contract::from_json(&once).expect("what this build wrote, this build reads");
    assert_eq!(
        contract, twice,
        "reading is not a fixed point, so `stamp` would rewrite bytes nobody asked it to"
    );
    assert_eq!(
        once,
        serde_json::to_string(&twice).expect("a read contract serialises"),
        "two serialisations of one document differ, so the round trip is not byte-stable"
    );
}
