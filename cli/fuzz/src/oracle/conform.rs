//! The rules, against the properties a rule set has to have to be worth running.
//!
//! `conform` is the one thing every future implementation is held to, so the question is not only
//! "does it crash" but "does it mean anything". Three properties, each of which a plausible
//! refactor breaks silently:
//!
//! **Determinism.** Two runs over one document agree, in the same order. A rule keyed on a hash
//! map's iteration order would pass every test in the suite and produce a different report on a
//! reviewer's machine than in CI, which is the fastest way to teach a team to ignore a gate.
//!
//! **Tier monotonicity.** Tier 2 is tier 1 *and more*, so its findings must be a superset. A tier
//! that dropped a refusal on the way up would let an implementation claim the higher one while
//! failing the lower.
//!
//! **Agreement with the meta-schema on the rules they share.** `contract.schema.json` encodes some
//! refusals structurally — a secret carrying a default is one — and where both have an opinion they
//! must not contradict each other. A document the schema accepts and the rules call malformed for
//! a reason the schema also encodes means one of the two has drifted.

use terrace_contract::conform::{self, Tier};
use terrace_contract::{Contract, validate};

use crate::mutate;

/// Run the oracle over one mutated document.
///
/// # Panics
/// On any finding.
pub fn check(data: &str) {
    let document = mutate::render(&mutate::document(data));

    // `validate` takes bytes, and takes them before anything is parsed — so it has to survive
    // every shape the reader would refuse, including the ones the mutator reaches by emptying a
    // required field.
    let schema_errors = validate::validate(&document).expect("the embedded meta-schema compiles");

    let Ok(contract) = Contract::from_json(&document) else {
        return;
    };

    let tier1 = conform::conform(&contract, Tier::Document);
    let tier2 = conform::conform(&contract, Tier::Dialect);
    let tier3 = conform::conform(&contract, Tier::Byte);

    assert_eq!(
        tier1,
        conform::conform(&contract, Tier::Document),
        "two runs over one document disagree, so the report depends on something that is not the \
         document"
    );

    assert!(
        tier2.starts_with(&tier1),
        "tier 2 is tier 1 and more, so its findings must extend tier 1's rather than replace them"
    );
    assert_eq!(
        tier3, tier2,
        "tier 3 adds no checkable-from-one-document rule, so it must report what tier 2 does"
    );

    // Every violation must name where it is. A finding with an empty location is one a producer's
    // author cannot act on, which makes it worse than silence — they will go looking and find
    // nothing.
    for violation in &tier2 {
        assert!(
            !violation.at.is_empty(),
            "a violation with no location: {violation}"
        );
        assert!(
            !violation.detail.is_empty(),
            "a violation with no detail: {violation}"
        );
    }

    // The rules and the meta-schema share exactly one refusal that both express structurally: a
    // secret carrying a default. Where both have an opinion they must not contradict each other,
    // or a producer's author is told different things by two halves of one gate.
    //
    // Asked of the *document* rather than of the validator's error text, which was the first
    // attempt and was wrong: matching on the words "default" and "null" also matches errors about
    // an unrelated field, and an oracle that fires on the wrong document is worse than one that
    // never fires at all.
    let secret_with_a_default = contract
        .schema
        .keys
        .iter()
        .any(|key| key.secret && key.default_value.is_some())
        || contract
            .external
            .env
            .iter()
            .any(|var| var.secret && var.default.is_some());

    if secret_with_a_default {
        assert!(
            tier1.iter().any(|violation| violation.rule == "refusal 6"),
            "a document carries a secret with a default and the rules did not say so"
        );
        assert!(
            !schema_errors.is_empty(),
            "a document carries a secret with a default and the meta-schema accepted it; the two halves of one gate have drifted apart"
        );
    }

    // A document that satisfies the meta-schema and every rule is one this build claims conforms.
    // The stored cases must stay in that set whatever the mutator did to *other* documents, which
    // is what stops an over-eager rule from being introduced quietly.
    for name in mutate::case_names() {
        let stored = serde_json::to_string(&mutate::stored(name)).expect("serialises");
        let stored = Contract::from_json(&stored).expect("a stored case reads");
        assert!(
            conform::conform(&stored, Tier::Dialect).is_empty(),
            "`{name}` stopped conforming at tier 2, so a rule introduced here is wrong"
        );
    }
}
