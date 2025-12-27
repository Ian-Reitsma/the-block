#![cfg(feature = "integration-tests")]
mod settlement_util;
mod util;
use settlement_util::SettlementCtx;
use the_block::compute_market::{admission, settlement, Offer};

#[test]
fn mixed_split_escrow_and_settlement() {
    let _ctx = SettlementCtx::new();
    let mut bal_consumer = 100;
    let mut bal_industrial = 100;
    let offer = Offer {
        job_id: "job1".into(),
        provider: "prov1".into(),
        provider_bond: 1,
        consumer_bond: 1,
        units: 1,
        price_per_unit: 20,
        fee_pct: 25,
        capability: the_block::compute_market::scheduler::Capability::default(),
        reputation: 0,
        reputation_multiplier: 1.0,
    };
    offer.validate().unwrap();
    let (consumer, industrial) = admission::reserve(
        &mut bal_consumer,
        &mut bal_industrial,
        offer.price_per_unit,
        offer.fee_pct,
    )
    .unwrap();
    assert_eq!((consumer, industrial), (5, 15));
    assert_eq!((bal_consumer, bal_industrial), (95, 85));
    settlement::Settlement::accrue_split(&offer.provider, consumer, industrial);
    assert_eq!(settlement::Settlement::balance(&offer.provider), 20);
}

#[test]
fn full_consumer_split_and_refund() {
    let _ctx = SettlementCtx::new();
    let mut bal_consumer = 50;
    let mut bal_industrial = 50;
    let offer = Offer {
        job_id: "job2".into(),
        provider: "prov2".into(),
        provider_bond: 1,
        consumer_bond: 1,
        units: 1,
        price_per_unit: 10,
        fee_pct: 100,
        capability: the_block::compute_market::scheduler::Capability::default(),
        reputation: 0,
        reputation_multiplier: 1.0,
    };
    offer.validate().unwrap();
    let (consumer, industrial) = admission::reserve(
        &mut bal_consumer,
        &mut bal_industrial,
        offer.price_per_unit,
        offer.fee_pct,
    )
    .unwrap();
    assert_eq!((consumer, industrial), (10, 0));
    assert_eq!((bal_consumer, bal_industrial), (40, 50));
    // simulate refund of unused escrow
    settlement::Settlement::refund_split("buyer2", consumer, industrial);
    assert_eq!(settlement::Settlement::balance("buyer2"), 10);
}
