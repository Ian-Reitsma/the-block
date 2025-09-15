use bridges::is_htlc_output;
use dex::htlc_router::{HtlcIntent, HtlcRouter};

#[test]
fn detects_valid_htlc_scripts() {
    let a = HtlcIntent { chain: "a".into(), amount: 1, hash: vec![1;32], timeout: 10 };
    let b = HtlcIntent { chain: "b".into(), amount: 1, hash: vec![1;32], timeout: 10 };
    let (s1, _) = HtlcRouter::generate_scripts(&a, &b);
    assert!(is_htlc_output(&s1));

    let a2 = HtlcIntent { chain: "a".into(), amount: 1, hash: vec![2;20], timeout: 5 };
    let b2 = HtlcIntent { chain: "b".into(), amount: 1, hash: vec![2;20], timeout: 5 };
    let (s2, _) = HtlcRouter::generate_scripts(&a2, &b2);
    assert!(is_htlc_output(&s2));

    assert!(!is_htlc_output(b"htlc:zz:abc"));
}
