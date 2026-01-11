use storage::{StorageContract, StorageOffer};
use storage::merkle_proof::MerkleTree;

use the_block::{rpc, telemetry};

fn demo_chunks() -> Vec<Vec<u8>> {
    vec![
        b"chunk0".to_vec(),
        b"chunk1".to_vec(),
        b"chunk2".to_vec(),
        b"chunk3".to_vec(),
    ]
}

fn sample_contract_with_root() -> (StorageContract, Vec<Vec<u8>>, MerkleTree) {
    let chunks = demo_chunks();
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
    let tree = MerkleTree::build(&chunk_refs).expect("build tree");
    let contract = StorageContract {
        object_id: "obj".into(),
        provider_id: "prov".into(),
        original_bytes: 1024,
        shares: 4,
        price_per_block: 1,
        start_block: 0,
        retention_blocks: 10,
        next_payment_block: 1,
        accrued: 0,
        total_deposit: 0,
        last_payment_block: None,
        storage_root: tree.root,
    };
    (contract, chunks, tree)
}

#[test]
fn storage_contract_lifecycle() {
    let (contract, _chunks, _tree) = sample_contract_with_root();
    assert!(contract.is_active(5).is_ok());
    assert!(contract.is_active(20).is_err());
}

#[test]
fn retrieval_challenge_and_slash() {
    let (contract, chunks, tree) = sample_contract_with_root();
    let offer = StorageOffer::new("provA".into(), 4096, 1, 10);
    rpc::storage::upload(contract.clone(), vec![offer]);
    let chunk_idx = 0;
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
    let proof = tree
        .generate_proof(chunk_idx, &chunk_refs)
        .expect("generate proof");
    let chunk_data = &chunks[chunk_idx as usize];
    let ok = rpc::storage::challenge(
        &contract.object_id,
        None,
        chunk_idx,
        chunk_data,
        &proof,
        5,
    );
    assert_eq!(ok["status"], "ok");
    assert_eq!(telemetry::RETRIEVAL_SUCCESS_TOTAL.value(), 1);
    let bad = rpc::storage::challenge(
        &contract.object_id,
        None,
        chunk_idx,
        b"wrong".as_ref(),
        &proof,
        5,
    );
    assert_eq!(bad["error"], "challenge_failed");
    assert_eq!(telemetry::RETRIEVAL_FAILURE_TOTAL.value(), 1);
}

#[test]
fn payments_accrue() {
    let chunks = demo_chunks();
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
    let tree = MerkleTree::build(&chunk_refs).expect("build tree");
    let mut contract = StorageContract {
        object_id: "p".into(),
        provider_id: "prov".into(),
        original_bytes: 0,
        shares: 0,
        price_per_block: 3,
        start_block: 0,
        retention_blocks: 4,
        next_payment_block: 1,
        accrued: 0,
        total_deposit: 0,
        last_payment_block: None,
        storage_root: tree.root,
    };
    assert_eq!(contract.pay(2), 6);
    assert_eq!(contract.pay(5), 6);
    assert_eq!(contract.accrued, 12);
}
