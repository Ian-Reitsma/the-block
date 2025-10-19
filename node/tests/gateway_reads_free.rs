#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_serialization::json::{Map, Value};
use sys::tempfile::tempdir;
use the_block::gateway::dns::{gateway_policy, publish_record};

#[test]
fn reads_increment_without_charging() {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_DNS_DB_PATH", dir.path().join("dns").to_str().unwrap());
    let mut policy = Map::new();
    policy.insert("gw_policy".to_string(), Value::Object(Map::new()));
    let txt = foundation_serialization::json::to_string_value(&Value::Object(policy));
    let sk = SigningKey::from_bytes(&[1u8; 32]);
    let pk = sk.verifying_key();
    let mut msg = Vec::new();
    msg.extend(b"test.block");
    msg.extend(txt.as_bytes());
    let sig = sk.sign(&msg);
    let mut params = Map::new();
    params.insert("domain".to_string(), Value::String("test.block".to_string()));
    params.insert("txt".to_string(), Value::String(txt.clone()));
    params.insert(
        "pubkey".to_string(),
        Value::String(crypto_suite::hex::encode(pk.to_bytes())),
    );
    params.insert(
        "sig".to_string(),
        Value::String(crypto_suite::hex::encode(sig.to_bytes())),
    );
    let _ = publish_record(&Value::Object(params));

    let mut domain = Map::new();
    domain.insert("domain".to_string(), Value::String("test.block".to_string()));
    let domain_val = Value::Object(domain.clone());
    let r1 = gateway_policy(&domain_val);
    assert_eq!(r1["reads_total"].as_u64().unwrap(), 1);
    assert!(r1.get("remaining_budget_μc").is_none());
    let r2 = gateway_policy(&domain_val);
    assert_eq!(r2["reads_total"].as_u64().unwrap(), 2);
    assert!(r2["last_access_ts"].as_u64().unwrap() >= r1["last_access_ts"].as_u64().unwrap());
}
