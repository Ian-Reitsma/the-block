use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use tempfile::tempdir;
use the_block::gateway::dns::{gateway_policy, publish_record};

#[test]
fn read_fee_deducts_budget() {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_DNS_DB_PATH", dir.path().join("dns").to_str().unwrap());
    let txt = json!({
        "gw_policy": {"budget_μc": 100, "read_fee_μc": 10, "credit_offset": 0}
    })
    .to_string();
    let sk = SigningKey::from_bytes(&[1u8; 32]);
    let pk = sk.verifying_key();
    let mut msg = Vec::new();
    msg.extend(b"test");
    msg.extend(txt.as_bytes());
    let sig = sk.sign(&msg);
    let params = json!({
        "domain":"test",
        "txt":txt,
        "pubkey":hex::encode(pk.to_bytes()),
        "sig":hex::encode(sig.to_bytes()),
    });
    let _ = publish_record(&params);
    let r1 = gateway_policy(&json!({"domain":"test"}));
    assert_eq!(r1["remaining_budget_μc"].as_u64().unwrap(), 90);
    let r2 = gateway_policy(&json!({"domain":"test"}));
    assert_eq!(r2["remaining_budget_μc"].as_u64().unwrap(), 80);
}
