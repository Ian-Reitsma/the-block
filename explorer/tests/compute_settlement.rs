use explorer::{compute_view, Explorer, ProviderSettlementRecord};
use tempfile::tempdir;

#[test]
fn provider_balances_round_trip() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Explorer::open(&db_path).expect("open explorer");
    let records = vec![ProviderSettlementRecord {
        provider: "alice".to_string(),
        ct: 42,
        industrial: 7,
        updated_at: 123,
    }];
    explorer
        .index_settlement_balances(&records)
        .expect("index balances");
    let stored = compute_view::provider_balances(&explorer).expect("fetch balances");
    assert_eq!(stored, records);
}
