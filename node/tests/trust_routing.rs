use the_block::dex::TrustLedger;

#[test]
fn primary_and_fallback_paths() {
    let mut ledger = TrustLedger::default();
    ledger.establish("a".into(), "c".into(), 100);
    ledger.authorize("a", "c");
    ledger.establish("a".into(), "b".into(), 100);
    ledger.authorize("a", "b");
    ledger.establish("b".into(), "c".into(), 100);
    ledger.authorize("b", "c");
    let (primary, fallback) = ledger.find_best_path("a", "c", 10).unwrap();
    assert_eq!(primary, vec!["a".to_string(), "c".to_string()]);
    assert_eq!(
        fallback.unwrap(),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}
