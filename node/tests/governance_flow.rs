#![allow(clippy::unwrap_used)]
use the_block::governance::{BicameralGovernance as Governance, House};

#[test]
fn submit_vote_exec_cycle() {
    let mut gov = Governance::new(1, 1, 0);
    let id = gov.submit(0, 0);
    gov.vote(id, House::Operators, true).unwrap();
    gov.vote(id, House::Builders, true).unwrap();
    assert!(gov.execute(id, 0).is_ok());
    let (p, remaining) = gov.status(id, 0).unwrap();
    assert!(p.executed);
    assert_eq!(remaining, 0);
}

#[test]
fn status_reports_timelock() {
    let mut gov = Governance::new(1, 1, 5);
    let id = gov.submit(0, 1);
    gov.vote(id, House::Operators, true).unwrap();
    gov.vote(id, House::Builders, true).unwrap();
    let (_, remaining) = gov.status(id, 3).unwrap();
    assert_eq!(remaining, 3);
}
