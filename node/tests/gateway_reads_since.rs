#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::{ed25519::SigningKey, Signer};
use tempfile::tempdir;
use the_block::gateway::dns::{gateway_policy, publish_record, reads_since};

#[test]
fn reads_since_reports_receipts() {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_DNS_DB_PATH", dir.path().join("dns").to_str().unwrap());
    std::env::set_var("TB_GATEWAY_RECEIPTS", dir.path());
    let txt = foundation_serialization::json::to_string_value(
        &foundation_serialization::json!({"gw_policy": {}}),
    );
    let sk = SigningKey::from_bytes(&[1u8; 32]);
    let pk = sk.verifying_key();
    let mut msg = Vec::new();
    msg.extend(b"test.block");
    msg.extend(txt.as_bytes());
    let sig = sk.sign(&msg);
    let params = foundation_serialization::json!({
        "domain":"test.block",
        "txt":txt,
        "pubkey":hex::encode(pk.to_bytes()),
        "sig":hex::encode(sig.to_bytes()),
    });
    let _ = publish_record(&params);
    let _ = gateway_policy(&foundation_serialization::json!({"domain":"test.block"}));
    let _ = gateway_policy(&foundation_serialization::json!({"domain":"test.block"}));
    let r = reads_since(&foundation_serialization::json!({"domain":"test.block","epoch":0}));
    assert_eq!(r["reads_total"].as_u64().unwrap(), 2);
}
