use the_block::compute_market::*;

#[test]
fn offer_validation() {
    let offer = Offer {
        job_id: "job".into(),
        provider_bond: 1,
        consumer_bond: 1,
        capacity: 5,
        price: 2,
    };
    assert!(offer.validate().is_ok());
}

#[test]
fn slice_proof_verification() {
    let data = b"slice";
    let mut h = blake3::Hasher::new();
    h.update(data);
    let hash = *h.finalize().as_bytes();
    let proof = SliceProof {
        reference: hash,
        output: hash,
        payout: 1,
    };
    assert!(proof.verify());
}

#[test]
fn price_band_and_adjustment() {
    let bands = price_bands(&[1, 2, 3, 4, 5]).unwrap();
    assert_eq!(bands, (2, 3, 4));
    assert_eq!(adjust_price(bands.1, 1.5), 5);
}

#[test]
fn courier_receipt_forwarding() {
    use the_block::compute_market::courier::CourierStore;
    let dir = tempfile::tempdir().unwrap();
    let store = CourierStore::open(dir.path().to_str().unwrap());
    let receipt = store.send(b"bundle", "alice");
    assert!(!receipt.acknowledged);
    let forwarded = store.flush(|r| r.sender == "alice").unwrap();
    assert_eq!(forwarded, 1);
    let rec = store.get(receipt.id).unwrap();
    assert!(rec.acknowledged);
}

#[test]
fn market_job_flow_and_finalize() {
    let mut market = Market::new();
    let offer = Offer {
        job_id: "job1".into(),
        provider_bond: 1,
        consumer_bond: 1,
        capacity: 1,
        price: 5,
    };
    market.post_offer(offer).unwrap();

    let mut h = blake3::Hasher::new();
    h.update(b"input");
    let ref_hash = *h.finalize().as_bytes();
    let job = Job {
        job_id: "job1".into(),
        slices: vec![ref_hash],
        price_per_slice: 5,
        consumer_bond: 1,
        workloads: vec![Workload::Transcode(b"input".to_vec())],
    };
    market.submit_job(job).unwrap();
    let proof = SliceProof {
        reference: ref_hash,
        output: ref_hash,
        payout: 5,
    };
    let payout = market.submit_slice("job1", proof).unwrap();
    assert_eq!(payout, 5);
    assert!(market.finalize_job("job1").is_some());
}

#[test]
fn price_board_tracks_bands() {
    let mut board = PriceBoard::default();
    for p in [1, 2, 3, 4, 5] {
        board.record(p);
    }
    assert_eq!(board.bands().unwrap(), (2, 3, 4));
}

#[test]
fn receipt_validation() {
    use the_block::compute_market::courier::CourierStore;
    let dir = tempfile::tempdir().unwrap();
    let store = CourierStore::open(dir.path().to_str().unwrap());
    store.send(b"payload", "bob");
    assert_eq!(store.flush(|_| false).unwrap(), 0);
    assert_eq!(store.flush(|r| r.sender == "bob").unwrap(), 1);
}
