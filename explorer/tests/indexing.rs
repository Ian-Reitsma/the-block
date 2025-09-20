use explorer::Explorer;
use tempfile::tempdir;
use the_block::compute_market::receipt::Receipt;

#[test]
fn ingest_and_query() {
    let dir = tempdir().unwrap();
    let receipts = dir.path().join("pending");
    std::fs::create_dir_all(&receipts).unwrap();
    let r = Receipt::new(
        "job".into(),
        "buyer".into(),
        "prov".into(),
        10,
        1,
        false,
        the_block::transaction::FeeLane::Consumer,
    );
    let bytes = bincode::serialize(&vec![r]).unwrap();
    std::fs::write(receipts.join("1"), bytes).unwrap();
    let db = dir.path().join("explorer.db");
    let ex = Explorer::open(&db).unwrap();
    ex.ingest_dir(&receipts).unwrap();
    assert_eq!(ex.receipts_by_provider("prov").unwrap().len(), 1);
    assert_eq!(ex.receipts_by_domain("buyer").unwrap().len(), 1);
}
