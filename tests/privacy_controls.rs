use privacy::redaction::redact_memo;

#[test]
fn memo_redaction() {
    let mut memo = "secret".to_string();
    assert!(redact_memo(&mut memo, false));
    assert!(memo.is_empty());
}

#[test]
fn memo_allowed() {
    let mut memo = "public".to_string();
    assert!(!redact_memo(&mut memo, true));
    assert_eq!(memo, "public");
}
