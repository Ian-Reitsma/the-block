#![forbid(unsafe_code)]

use std::error::Error;

use storage::{merkle_proof::MerkleTree, StorageContract};
use storage_market::{ProofOutcome, ReplicaIncentive, StorageMarket};
use sys::tempfile::tempdir;

type TestResult<T> = Result<T, Box<dyn Error>>;

#[test]
fn registration_and_proof_flow_persists_through_engine() -> TestResult<()> {
    let dir = tempdir()?;
    let path = dir.path().join("market.db");

    let mut market = StorageMarket::open(&path)?;
    let chunks = vec![
        b"engine-chunk-0".to_vec(),
        b"engine-chunk-1".to_vec(),
        b"engine-chunk-2".to_vec(),
        b"engine-chunk-3".to_vec(),
    ];
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
    let tree = MerkleTree::build(&chunk_refs).expect("build tree");
    let contract = StorageContract {
        object_id: "obj-1".into(),
        provider_id: "primary".into(),
        original_bytes: 2_048,
        shares: 8,
        price_per_block: 10,
        start_block: 0,
        retention_blocks: 12,
        next_payment_block: 1,
        accrued: 0,
        total_deposit: 0,
        last_payment_block: None,
        storage_root: tree.root,
    };
    let replica_a = ReplicaIncentive::new("primary".into(), 8, 10, 100);
    let replica_b = ReplicaIncentive::new("backup".into(), 4, 10, 50);

    let registered = market.register_contract(contract, vec![replica_a, replica_b])?;
    assert_eq!(registered.contract.total_deposit, 150);

    let listing = market.contracts()?;
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].contract.object_id, "obj-1");

    let (success, _) = market.record_proof_outcome("obj-1", Some("primary"), 5, true, None)?;
    assert_eq!(success.outcome, ProofOutcome::Success);
    assert_eq!(success.amount_accrued, 40);
    assert_eq!(success.remaining_deposit, 100);

    let (failure, _) = market.record_proof_outcome("obj-1", Some("backup"), 6, false, None)?;
    assert_eq!(failure.outcome, ProofOutcome::Failure);
    assert_eq!(failure.slashed, 10);
    assert_eq!(failure.remaining_deposit, 40);
    assert_eq!(failure.amount_accrued, 40);

    drop(market);

    let reopened = StorageMarket::open(&path)?;
    let persisted = reopened
        .load_contract("obj-1")?
        .expect("contract should persist");
    assert_eq!(persisted.contract.total_deposit, 140);
    assert_eq!(
        persisted
            .replicas
            .iter()
            .find(|r| r.provider_id == "backup")
            .map(|r| r.deposit),
        Some(40)
    );

    reopened.clear()?;
    assert!(reopened.contracts()?.is_empty());

    Ok(())
}
