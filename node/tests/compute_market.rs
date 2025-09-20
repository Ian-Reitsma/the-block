#![cfg(feature = "integration-tests")]
use the_block::compute_market::courier_store::ReceiptStore;
use the_block::compute_market::matcher::{self, Ask, Bid, LaneMetadata, LaneSeed};
use the_block::compute_market::{price_board::PriceBoard, scheduler, ExecutionReceipt, *};
use the_block::transaction::FeeLane;
use tokio_util::sync::CancellationToken;

#[test]
fn offer_validation() {
    let offer = Offer {
        job_id: "job".into(),
        provider: "prov".into(),
        provider_bond: 1,
        consumer_bond: 1,
        units: 5,
        price_per_unit: 2,
        fee_pct_ct: 100,
        capability: scheduler::Capability::default(),
        reputation: 0,
        reputation_multiplier: 1.0,
    };
    assert!(offer.validate().is_ok());
}

#[test]
fn slice_proof_verification() {
    let data = b"slice";
    let mut h = blake3::Hasher::new();
    h.update(data);
    let hash = *h.finalize().as_bytes();
    let proof = ExecutionReceipt {
        reference: hash,
        output: hash,
        payout: 1,
        proof: None,
    };
    assert!(proof.verify(&Workload::Transcode(data.to_vec())));
}

#[test]
fn price_band_and_adjustment() {
    let bands = price_bands(&[1, 2, 3, 4, 5]).unwrap();
    assert_eq!(bands, (2, 3, 4));
    assert_eq!(adjust_price(bands.1, 1.5), 5);
}

#[test]
fn market_job_flow_and_finalize() {
    let mut market = Market::new();
    let offer = Offer {
        job_id: "job1".into(),
        provider: "prov".into(),
        provider_bond: 1,
        consumer_bond: 1,
        units: 1,
        price_per_unit: 5,
        fee_pct_ct: 100,
        capability: scheduler::Capability::default(),
        reputation: 0,
        reputation_multiplier: 1.0,
    };
    market.post_offer(offer).unwrap();

    let mut h = blake3::Hasher::new();
    h.update(b"input");
    let ref_hash = *h.finalize().as_bytes();
    let job = Job {
        job_id: "job1".into(),
        buyer: "buyer".into(),
        slices: vec![ref_hash],
        price_per_unit: 5,
        consumer_bond: 1,
        workloads: vec![Workload::Transcode(b"input".to_vec())],
        capability: scheduler::Capability::default(),
        deadline: u64::MAX,
        priority: scheduler::Priority::Normal,
    };
    market.submit_job(job).unwrap();
    let proof = ExecutionReceipt {
        reference: ref_hash,
        output: ref_hash,
        payout: 5,
        proof: None,
    };
    let payout = market.submit_slice("job1", proof).unwrap();
    assert_eq!(payout, 5);
    assert!(market.finalize_job("job1").is_some());
}

#[test]
fn price_board_tracks_bands() {
    let mut board = PriceBoard::new(5);
    for p in [1, 2, 3, 4, 5] {
        board.record(FeeLane::Consumer, p, 1.0);
    }
    assert_eq!(board.bands(FeeLane::Consumer).unwrap(), (2, 3, 4));
}

#[tokio::test]
async fn dry_run_receipts_are_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("receipts");
    let store = ReceiptStore::open(store_path.to_str().unwrap());
    matcher::seed_orders(vec![LaneSeed {
        lane: FeeLane::Consumer,
        bids: vec![Bid {
            job_id: "job".into(),
            buyer: "buyer".into(),
            price: 5,
            lane: FeeLane::Consumer,
        }],
        asks: vec![Ask {
            job_id: "job".into(),
            provider: "prov".into(),
            price: 5,
            lane: FeeLane::Consumer,
        }],
        metadata: LaneMetadata::default(),
    }])
    .unwrap();
    let stop = CancellationToken::new();
    let handle = tokio::spawn(matcher::match_loop(store.clone(), true, stop.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    stop.cancel();
    handle.await.unwrap();
    assert_eq!(store.len().unwrap(), 1);
    assert_eq!(store.recent_by_lane(FeeLane::Consumer, 4).unwrap().len(), 1);
    drop(store);

    let store = ReceiptStore::open(store_path.to_str().unwrap());
    matcher::seed_orders(vec![LaneSeed {
        lane: FeeLane::Consumer,
        bids: vec![Bid {
            job_id: "job".into(),
            buyer: "buyer".into(),
            price: 5,
            lane: FeeLane::Consumer,
        }],
        asks: vec![Ask {
            job_id: "job".into(),
            provider: "prov".into(),
            price: 5,
            lane: FeeLane::Consumer,
        }],
        metadata: LaneMetadata::default(),
    }])
    .unwrap();
    let stop = CancellationToken::new();
    let handle = tokio::spawn(matcher::match_loop(store.clone(), true, stop.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    stop.cancel();
    handle.await.unwrap();
    assert_eq!(store.len().unwrap(), 1);
    let receipts = store.recent_by_lane(FeeLane::Consumer, 4).unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].lane, FeeLane::Consumer);
    matcher::seed_orders(Vec::new()).unwrap();
}
