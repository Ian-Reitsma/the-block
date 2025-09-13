use storage::StorageContract;

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
    };
    assert!(contract.is_active(5).is_ok());
    assert!(contract.is_active(20).is_err());
}
