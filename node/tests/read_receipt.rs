use tempfile::tempdir;
use the_block::gateway::read_receipt::{append, reads_since};

#[test]
fn reads_since_counts_receipts() {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_GATEWAY_RECEIPTS", dir.path());
    append("ex.com", "prov1", 10, false, true).unwrap();
    let (total, last) = reads_since(0, "ex.com");
    assert_eq!(total, 1);
    assert!(last > 0);
}
