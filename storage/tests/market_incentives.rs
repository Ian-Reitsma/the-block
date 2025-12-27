#![cfg(test)]

use storage::{merkle_proof::MerkleTree, StorageContract};
use storage_market::{ReplicaIncentive, StorageMarket};
use sys::tempfile::tempdir;

fn test_contract() -> StorageContract {
    let chunks = vec![
        b"market-chunk-0".to_vec(),
        b"market-chunk-1".to_vec(),
        b"market-chunk-2".to_vec(),
    ];
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
    let tree = MerkleTree::build(&chunk_refs).expect("build tree");
    StorageContract {
        object_id: "obj".into(),
        provider_id: "prov".into(),
        original_bytes: 512,
        shares: 4,
        price_per_block: 6,
        start_block: 0,
        retention_blocks: 12,
        next_payment_block: 1,
        accrued: 0,
        total_deposit: 0,
        last_payment_block: None,
        storage_root: tree.root,
    }
}

#[test]
fn market_persists_contracts_and_records_proofs() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("market.db");
    let mut market = StorageMarket::open(&db_path).expect("open");
    let contract = test_contract();
    let replica = ReplicaIncentive::new("prov".into(), 4, 6, 72);
    market
        .register_contract(contract, vec![replica])
        .expect("register");

    market
        .record_proof_outcome("obj", None, 4, true)
        .expect("record success");
    market
        .record_proof_outcome("obj", None, 5, false)
        .expect("record failure");

    drop(market);
    let reopened = StorageMarket::open(&db_path).expect("reopen");
    let contracts = reopened.contracts().expect("contracts");
    assert_eq!(contracts.len(), 1);
    let record = &contracts[0];
    assert_eq!(record.contract.object_id, "obj");
    assert_eq!(record.contract.accrued, 18);
    assert_eq!(record.contract.total_deposit, record.replicas[0].deposit);
    assert_eq!(record.replicas[0].proof_successes, 1);
    assert_eq!(record.replicas[0].proof_failures, 1);
}
