use explorer::{Explorer, ReceiptRecord};
use sys::tempfile;

#[test]
fn ingest_and_query() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("explorer.db");
    let ex = Explorer::open(&db).unwrap();
    let rec = ReceiptRecord {
        key: "key-1".into(),
        epoch: 1,
        provider: "prov".into(),
        buyer: "buyer".into(),
        amount: 10,
        kernel_digest: None,
        descriptor_digest: None,
        output_digest: None,
        benchmark_commit: None,
        tensor_profile_epoch: None,
        proof_latency_ms: None,
    };
    ex.index_receipt(&rec).unwrap();
    assert_eq!(ex.receipts_by_provider("prov").unwrap().len(), 1);
    assert_eq!(ex.receipts_by_domain("buyer").unwrap().len(), 1);
}
