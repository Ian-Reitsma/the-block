#![cfg(feature = "integration-tests")]
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use tempfile::tempdir;
use the_block::gateway::dns::{gateway_policy, publish_record};

#[test]
fn reads_increment_without_charging() {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_DNS_DB_PATH", dir.path().join("dns").to_str().unwrap());
    let txt = json!({"gw_policy":{}}).to_string();
    let sk = SigningKey::from_bytes(&[1u8; 32]);
    let pk = sk.verifying_key();
    let mut msg = Vec::new();
    msg.extend(b"test.block");
    msg.extend(txt.as_bytes());
    let sig = sk.sign(&msg);
    let params = json!({
        "domain":"test.block",
        "txt":txt,
        "pubkey":hex::encode(pk.to_bytes()),
        "sig":hex::encode(sig.to_bytes()),
    });
    let _ = publish_record(&params);
    let r1 = gateway_policy(&json!({"domain":"test.block"}));
    assert_eq!(r1["reads_total"].as_u64().unwrap(), 1);
    assert!(r1.get("remaining_budget_μc").is_none());
    let r2 = gateway_policy(&json!({"domain":"test.block"}));
    assert_eq!(r2["reads_total"].as_u64().unwrap(), 2);
    assert!(r2["last_access_ts"].as_u64().unwrap() >= r1["last_access_ts"].as_u64().unwrap());
}
