#![allow(clippy::unwrap_used)]
use the_block::{Bicameral, Proposal};

#[test]
fn proposal_executes_after_quorum_and_timelock() {
    let mut p = Proposal::new(1, 0, 10);
    // reach quorum
    for _ in 0..3 {
        p.vote_operator(true);
        p.vote_builder(true);
    }
    let gov = Bicameral::new(3, 3, 5);
    assert!(!gov.can_execute(&p, 14));
    assert!(gov.can_execute(&p, 15));
}
