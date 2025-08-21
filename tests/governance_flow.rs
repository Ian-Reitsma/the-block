use the_block::governance::{Governance, House};

#[test]
fn proposal_lifecycle() {
    let mut gov = Governance::new(1, 1, 0);
    let id = gov.submit(0, 1);
    gov.vote(id, House::Operators, true).unwrap();
    gov.vote(id, House::Builders, true).unwrap();
    assert!(gov.execute(id, 2).is_ok());
}
