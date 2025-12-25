#![cfg(feature = "integration-tests")]
mod util;
use util::settlement::SettlementCtx;
use the_block::compute_market::{admission, settlement, Offer};

#[test]
fn mixed_split_escrow_and_settlement() {
    let _ctx = SettlementCtx::new();
    let mut bal_ct = 100;
    let mut bal_it = 100;
    let offer = Offer {
        job_id: "job1".into(),
        provider: "prov1".into(),
        provider_bond: 1,
        consumer_bond: 1,
        units: 1,
        price_per_unit: 20,
        fee_pct_ct: 25,
        capability: the_block::compute_market::scheduler::Capability::default(),
        reputation: 0,
        reputation_multiplier: 1.0,
    };
    offer.validate().unwrap();
    let (ct, it) = admission::reserve(
        &mut bal_ct,
        &mut bal_it,
        offer.price_per_unit,
        offer.fee_pct_ct,
    )
    .unwrap();
    assert_eq!((ct, it), (5, 15));
    assert_eq!((bal_ct, bal_it), (95, 85));
    settlement::Settlement::accrue_split(&offer.provider, ct, it);
    assert_eq!(settlement::Settlement::balance(&offer.provider), 20);
}

#[test]
fn full_ct_split_and_refund() {
    let _ctx = SettlementCtx::new();
    let mut bal_ct = 50;
    let mut bal_it = 50;
    let offer = Offer {
        job_id: "job2".into(),
        provider: "prov2".into(),
        provider_bond: 1,
        consumer_bond: 1,
        units: 1,
        price_per_unit: 10,
        fee_pct_ct: 100,
        capability: the_block::compute_market::scheduler::Capability::default(),
        reputation: 0,
        reputation_multiplier: 1.0,
    };
    offer.validate().unwrap();
    let (ct, it) = admission::reserve(
        &mut bal_ct,
        &mut bal_it,
        offer.price_per_unit,
        offer.fee_pct_ct,
    )
    .unwrap();
    assert_eq!((ct, it), (10, 0));
    assert_eq!((bal_ct, bal_it), (40, 50));
    // simulate refund of unused escrow
    settlement::Settlement::refund_split("buyer2", ct, it);
    assert_eq!(settlement::Settlement::balance("buyer2"), 10);
}
