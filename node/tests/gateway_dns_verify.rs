#![cfg(feature = "integration-tests")]
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use serial_test::serial;
use tempfile::tempdir;
use the_block::gateway::dns::{
    clear_verify_cache, dns_lookup, gateway_policy, publish_record, set_allow_external,
    set_txt_resolver,
};

fn setup(domain: &str) -> (tempfile::TempDir, String, SigningKey) {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_DNS_DB_PATH", dir.path().join("dns").to_str().unwrap());
    clear_verify_cache();
    set_allow_external(false);
    set_txt_resolver(|_| vec![]);
    let txt = json!({"gw_policy":{}}).to_string();
    let sk = SigningKey::from_bytes(&[1u8; 32]);
    let pk = sk.verifying_key();
    let mut msg = Vec::new();
    msg.extend(domain.as_bytes());
    msg.extend(txt.as_bytes());
    let sig = sk.sign(&msg);
    let params = json!({
        "domain":domain,
        "txt":txt,
        "pubkey":hex::encode(pk.to_bytes()),
        "sig":hex::encode(sig.to_bytes()),
    });
    let _ = publish_record(&params);
    (dir, hex::encode(pk.to_bytes()), sk)
}

#[test]
#[serial]
fn block_tld_trusted() {
    let (_dir, _pk_hex, _sk) = setup("good.block");
    let l = dns_lookup(&json!({"domain":"good.block"}));
    assert!(l["verified"].as_bool().unwrap());
    let p = gateway_policy(&json!({"domain":"good.block"}));
    assert!(p["record"].is_string());
}

#[test]
#[serial]
fn external_domain_verified() {
    let (_dir, pk_hex, _sk) = setup("example.com");
    set_allow_external(true);
    let pk_clone = pk_hex.clone();
    set_txt_resolver(move |_| vec![pk_clone.clone()]);
    let l = dns_lookup(&json!({"domain":"example.com"}));
    assert!(l["verified"].as_bool().unwrap());
    let p = gateway_policy(&json!({"domain":"example.com"}));
    assert!(p["record"].is_string());
}

#[test]
#[serial]
fn external_domain_rejected() {
    let (_dir, _pk_hex, _sk) = setup("bad.com");
    set_allow_external(true);
    set_txt_resolver(|_| vec!["other".into()]);
    let l = dns_lookup(&json!({"domain":"bad.com"}));
    assert!(!l["verified"].as_bool().unwrap());
    let p = gateway_policy(&json!({"domain":"bad.com"}));
    assert!(p["record"].is_null());
}
