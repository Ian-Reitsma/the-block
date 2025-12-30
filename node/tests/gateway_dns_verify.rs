#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_serialization::json::{Map, Value};
use sys::tempfile::tempdir;
use the_block::gateway::dns::{
    clear_verify_cache, dns_lookup, gateway_policy, publish_record, set_allow_external,
    set_txt_resolver,
};

fn setup(domain: &str) -> (sys::tempfile::TempDir, String, SigningKey) {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_DNS_DB_PATH", dir.path().join("dns").to_str().unwrap());
    clear_verify_cache();
    set_allow_external(false);
    set_txt_resolver(|_| vec![]);
    let mut policy = Map::new();
    policy.insert("gw_policy".to_string(), Value::Object(Map::new()));
    let txt = foundation_serialization::json::to_string_value(&Value::Object(policy));
    let sk = SigningKey::from_bytes(&[1u8; 32]);
    let pk = sk.verifying_key();
    let mut msg = Vec::new();
    msg.extend(domain.as_bytes());
    msg.extend(txt.as_bytes());
    let sig = sk.sign(&msg);
    let mut params = Map::new();
    params.insert("domain".to_string(), Value::String(domain.to_string()));
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
    (dir, crypto_suite::hex::encode(pk.to_bytes()), sk)
}

#[testkit::tb_serial]
fn block_tld_trusted() {
    let (_dir, _pk_hex, _sk) = setup("good.block");
    let mut domain = Map::new();
    domain.insert(
        "domain".to_string(),
        Value::String("good.block".to_string()),
    );
    let lookup_req = Value::Object(domain.clone());
    let l = dns_lookup(&lookup_req);
    assert!(l["verified"].as_bool().unwrap());
    let p = gateway_policy(&lookup_req);
    assert!(matches!(p["record"], Value::String(_)));
}

#[testkit::tb_serial]
fn external_domain_verified() {
    let (_dir, pk_hex, _sk) = setup("example.com");
    set_allow_external(true);
    let pk_clone = pk_hex.clone();
    // TXT records must be in format "tb-verification={node_id}" per DNS_VERIFICATION_PREFIX constant
    set_txt_resolver(move |_| vec![format!("tb-verification={}", pk_clone)]);
    let mut domain = Map::new();
    domain.insert(
        "domain".to_string(),
        Value::String("example.com".to_string()),
    );
    let lookup_req = Value::Object(domain.clone());
    let l = dns_lookup(&lookup_req);
    assert!(l["verified"].as_bool().unwrap());
    let p = gateway_policy(&lookup_req);
    assert!(matches!(p["record"], Value::String(_)));

    // Comprehensive cleanup: reset all global state to prevent test pollution
    set_allow_external(false);
    set_txt_resolver(|_| vec![]);
    clear_verify_cache();
}

#[testkit::tb_serial]
fn external_domain_rejected() {
    let (_dir, _pk_hex, _sk) = setup("bad.com");
    set_allow_external(true);
    set_txt_resolver(|_| vec!["other".into()]);
    let mut domain = Map::new();
    domain.insert("domain".to_string(), Value::String("bad.com".to_string()));
    let lookup_req = Value::Object(domain.clone());
    let l = dns_lookup(&lookup_req);
    assert!(!l["verified"].as_bool().unwrap());
    let p = gateway_policy(&lookup_req);
    assert!(matches!(p["record"], Value::Null));
}
