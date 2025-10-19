#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_serialization::json::{Map, Number, Value};
use sys::tempfile::tempdir;
use the_block::gateway::dns::{gateway_policy, publish_record, reads_since};

#[test]
fn reads_since_reports_receipts() {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_DNS_DB_PATH", dir.path().join("dns").to_str().unwrap());
    std::env::set_var("TB_GATEWAY_RECEIPTS", dir.path());
    let mut policy_obj = Map::new();
    policy_obj.insert("gw_policy".to_string(), Value::Object(Map::new()));
    let txt = foundation_serialization::json::to_string_value(&Value::Object(policy_obj));
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
    let _ = gateway_policy(&domain_val);
    let _ = gateway_policy(&domain_val);

    let mut reads_params = domain;
    reads_params.insert("epoch".to_string(), Value::Number(Number::from(0))); 
    let r = reads_since(&Value::Object(reads_params));
    assert_eq!(r["reads_total"].as_u64().unwrap(), 2);
}
