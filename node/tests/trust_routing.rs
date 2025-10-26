#![cfg(feature = "integration-tests")]
use the_block::dex::TrustLedger;

#[test]
fn primary_and_fallback_paths() {
    let mut ledger = TrustLedger::default();
    ledger.establish("a".into(), "c".into(), 30);
    ledger.establish("c".into(), "a".into(), 30);
    ledger.authorize("a", "c");
    ledger.authorize("c", "a");
    assert!(ledger.adjust("a", "c", 10));
    ledger.establish("a".into(), "b".into(), 100);
    ledger.authorize("a", "b");
    ledger.establish("b".into(), "a".into(), 100);
    ledger.authorize("b", "a");
    ledger.establish("b".into(), "c".into(), 100);
    ledger.authorize("b", "c");
    ledger.establish("c".into(), "b".into(), 100);
    ledger.authorize("c", "b");
    let (primary, fallback) = ledger.find_best_path("a", "c", 10).unwrap();
    assert_eq!(
        primary,
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
    assert_eq!(fallback.unwrap(), vec!["a".to_string(), "c".to_string()]);
}
