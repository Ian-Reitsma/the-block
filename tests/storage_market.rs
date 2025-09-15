use storage::{StorageContract, StorageOffer};

use the_block::{rpc, telemetry};

#[test]
fn storage_contract_lifecycle() {
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
    };
    assert!(contract.is_active(5).is_ok());
    assert!(contract.is_active(20).is_err());
}

#[test]
fn retrieval_challenge_and_slash() {
    let contract = StorageContract {
        object_id: "obj2".into(),
        provider_id: "provA".into(),
        original_bytes: 2048,
        shares: 2,
        price_per_block: 1,
        start_block: 0,
        retention_blocks: 10,
        next_payment_block: 1,
        accrued: 0,
    };
    let offer = StorageOffer::new("provA".into(), 4096, 1, 10);
    rpc::storage::upload(contract.clone(), vec![offer]);
    let proof = contract.expected_proof(0);
    let ok = rpc::storage::challenge(&contract.object_id, 0, proof, 5);
    assert_eq!(ok["status"], "ok");
    assert_eq!(telemetry::RETRIEVAL_SUCCESS_TOTAL.get(), 1);
    let bad = rpc::storage::challenge(&contract.object_id, 0, [0u8; 32], 5);
    assert_eq!(bad["error"], "challenge_failed");
    assert_eq!(telemetry::RETRIEVAL_FAILURE_TOTAL.get(), 1);
}

#[test]
fn payments_accrue() {
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
    };
    assert_eq!(contract.pay(2), 6);
    assert_eq!(contract.pay(5), 6);
    assert_eq!(contract.accrued, 12);
}
