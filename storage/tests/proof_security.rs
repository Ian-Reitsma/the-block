//! Security tests for storage proof system
//!
//! These tests verify that the Merkle proof system actually prevents
//! providers from faking data possession.

use storage::contract::{ContractError, StorageContract};
use storage::merkle_proof::MerkleTree;

#[test]
fn cannot_prove_without_data() {
    // Setup: Create a contract with actual chunks
    let chunks = vec![b"chunk0", b"chunk1", b"chunk2", b"chunk3"];
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|c| c.as_ref()).collect();
    let tree = MerkleTree::build(&chunk_refs).expect("build tree");

    let contract = StorageContract {
        object_id: "object_001".into(),
        provider_id: "provider_001".into(),
        original_bytes: 4000,
        shares: 4,
        price_per_block: 100,
        start_block: 0,
        retention_blocks: 1000,
        next_payment_block: 1,
        accrued: 0,
        total_deposit_ct: 100000,
        last_payment_block: None,
        storage_root: tree.root,
    };

    // Attack: Provider tries to compute proof without storing data
    // They only have meta object_id, chunk_idx
    let chunk_idx = 1u64;

    // Attempt 1: Use wrong chunk data
    let fake_chunk = b"I dont have the real data";
    let fake_proof = tree
        .generate_proof(chunk_idx, &chunk_refs)
        .expect("generate proof");

    let result = contract.verify_proof(chunk_idx, fake_chunk, &fake_proof, 100);

    // Must fail - wrong data
    assert!(matches!(result, Err(ContractError::ChallengeFailed)));

    // Attempt 2: Try to forge proof without actual chunk
    // Even with valid metadata, can't forge Merkle path
    use storage::merkle_proof::MerkleProof;
    let forged_proof = MerkleProof::new(vec![0u8; 96]).expect("forge proof"); // 3 levels

    let result = contract.verify_proof(chunk_idx, b"guessed_data", &forged_proof, 100);

    assert!(matches!(result, Err(ContractError::ChallengeFailed)));

    // Success: Provider with actual data passes
    let real_chunk = chunks[chunk_idx as usize];
    let real_proof = tree
        .generate_proof(chunk_idx, &chunk_refs)
        .expect("real proof");

    let result = contract.verify_proof(chunk_idx, real_chunk, &real_proof, 100);

    assert!(result.is_ok());
}

#[test]
fn proof_for_wrong_index_fails() {
    let chunks = vec![b"chunk0", b"chunk1", b"chunk2", b"chunk3"];
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|c| c.as_ref()).collect();

    let tree = MerkleTree::build(&chunk_refs).expect("build tree");

    let contract = StorageContract {
        object_id: "object_001".into(),
        provider_id: "provider_001".into(),
        original_bytes: 4000,
        shares: 4,
        price_per_block: 100,
        start_block: 0,
        retention_blocks: 1000,
        next_payment_block: 1,
        accrued: 0,
        total_deposit_ct: 100000,
        last_payment_block: None,
        storage_root: tree.root,
    };

    // Generate proof for chunk 1
    let proof_for_1 = tree.generate_proof(1, &chunk_refs).expect("proof for 1");

    // Try to use it for chunk 2
    let result = contract.verify_proof(
        2,         // Wrong index
        chunks[1], // Data from chunk 1
        &proof_for_1,
        100,
    );

    assert!(matches!(result, Err(ContractError::ChallengeFailed)));
}

#[test]
fn modified_chunk_detected() {
    let chunks = vec![b"chunk0", b"chunk1", b"chunk2", b"chunk3"];
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|c| c.as_ref()).collect();

    let tree = MerkleTree::build(&chunk_refs).expect("build tree");

    let contract = StorageContract {
        object_id: "object_001".into(),
        provider_id: "provider_001".into(),
        original_bytes: 4000,
        shares: 4,
        price_per_block: 100,
        start_block: 0,
        retention_blocks: 1000,
        next_payment_block: 1,
        accrued: 0,
        total_deposit_ct: 100000,
        last_payment_block: None,
        storage_root: tree.root,
    };

    let chunk_idx = 2u64;
    let proof = tree.generate_proof(chunk_idx, &chunk_refs).expect("proof");

    // Provider modified the chunk
    let modified_chunk = b"MODIFIED";

    let result = contract.verify_proof(chunk_idx, modified_chunk, &proof, 100);

    assert!(matches!(result, Err(ContractError::ChallengeFailed)));
}

#[test]
fn expired_contract_rejects_proofs() {
    let chunks = vec![b"chunk0"];
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|c| c.as_ref()).collect();

    let tree = MerkleTree::build(&chunk_refs).expect("build tree");

    let contract = StorageContract {
        object_id: "object_001".into(),
        provider_id: "provider_001".into(),
        original_bytes: 1000,
        shares: 1,
        price_per_block: 100,
        start_block: 100,
        retention_blocks: 50, // Expires at block 150
        next_payment_block: 101,
        accrued: 0,
        total_deposit_ct: 5000,
        last_payment_block: None,
        storage_root: tree.root,
    };

    let proof = tree.generate_proof(0, &chunk_refs).expect("proof");

    // Try to prove at block 200 (after expiry)
    let result = contract.verify_proof(0, chunks[0], &proof, 200);

    assert!(matches!(result, Err(ContractError::Expired)));
}

#[test]
fn proof_size_attack_prevented() {
    // Attacker tries to DoS by submitting huge proof
    use storage::merkle_proof::{MerkleError, MerkleProof};

    // Try to create giant proof (10MB of fake siblings)
    let giant_proof_data = vec![0u8; 10_000_000];
    let giant_proof = MerkleProof::new(giant_proof_data);

    assert!(matches!(
        giant_proof,
        Err(MerkleError::InvalidProofLength {
            expected: _,
            got: _
        })
    ));
}

#[test]
fn random_challenges_unpredictable() {
    // Verify that different chunk indices require different proofs
    let chunks = vec![
        b"chunk0", b"chunk1", b"chunk2", b"chunk3", b"chunk4", b"chunk5", b"chunk6", b"chunk7",
    ];
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|c| c.as_ref()).collect();

    let tree = MerkleTree::build(&chunk_refs).expect("build tree");

    let contract = StorageContract {
        object_id: "object_001".into(),
        provider_id: "provider_001".into(),
        original_bytes: 8000,
        shares: 8,
        price_per_block: 100,
        start_block: 0,
        retention_blocks: 1000,
        next_payment_block: 1,
        accrued: 0,
        total_deposit_ct: 100000,
        last_payment_block: None,
        storage_root: tree.root,
    };

    // Generate proofs for all chunks
    let mut proofs = Vec::new();
    for i in 0..8 {
        let proof = tree.generate_proof(i, &chunk_refs).expect("proof");
        proofs.push(proof);
    }

    // Verify each proof only works for its corresponding chunk
    for i in 0..8 {
        // Correct proof passes
        assert!(
            contract
                .verify_proof(i, chunks[i as usize], &proofs[i as usize], 100)
                .is_ok()
        );

        // Wrong chunk fails
        if i + 1 < 8 {
            let wrong_idx = (i + 1) % 8;
            assert!(
                contract
                    .verify_proof(i, chunks[wrong_idx as usize], &proofs[i as usize], 100)
                    .is_err()
            );
        }
    }
}
